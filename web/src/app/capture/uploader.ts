import { BehaviorSubject, Observable } from 'rxjs';

import { ChunkStore } from './chunk-store';

/** One chunk's presigned upload URL. The URL is a bearer credential: never log it. */
export interface PresignedUrl {
  idx: number;
  url: string;
}

/** What the server has for a take (`GET /takes/{id}/status`). */
export interface UploadStatus {
  finalized: boolean;
  chunks: { idx: number; size_bytes: number; sha256: string }[];
}

/**
 * The ingest endpoints the uploader needs (upload protocol v1, docs/design.md §9). The app
 * implements it over HTTP; tests and the extension supply their own. Failures should be
 * `UploadHttpError`s so the uploader can tell a dropped connection from a refusal.
 */
export interface UploadApi {
  presign(takeId: string, idx: number, count: number): Promise<PresignedUrl[]>;
  ack(takeId: string, idx: number, sizeBytes: number, sha256: string): Promise<void>;
  status(takeId: string): Promise<UploadStatus>;
  /** Declares the take complete; the server answers `422` if chunks are missing. */
  finalize(takeId: string, chunkCount: number, durationMs: number): Promise<FinalizedTake>;
}

export interface FinalizedTake {
  recording_id: string;
}

/** Sends a chunk's bytes to its presigned URL. */
export type PutChunk = (url: string, blob: Blob) => Promise<void>;

/** Hashes a chunk: lowercase hex SHA-256. */
export type DigestChunk = (blob: Blob) => Promise<string>;

/** Whether the browser thinks it's online, and a way to wait until it is. */
export interface NetworkPort {
  isOnline(): boolean;
  whenOnline(): Promise<void>;
}

export type UploaderState =
  'idle' | 'uploading' | 'retrying' | 'offline' | 'finalizing' | 'finalized' | 'failed';

export interface UploadProgress {
  /** Chunks enqueued so far (uploaded or not). */
  queued: number;
  /** Chunks uploaded and acknowledged. */
  uploaded: number;
  state: UploaderState;
}

export interface UploaderOptions {
  api: UploadApi;
  /** Where the chunks are: every chunk is in the store before it is uploaded (US-12). */
  store: ChunkStore;
  /** The server's take id. */
  takeId: string;
  /** The key the chunks are stored under, when it isn't `takeId` (a take recorded offline). */
  storeTakeId?: string;
  put?: PutChunk;
  digest?: DigestChunk;
  network?: NetworkPort;
  /** Waits `ms` (tests pass an instant one). */
  delay?: (ms: number) => Promise<void>;
  /** 0 ≤ n < 1, for backoff jitter. */
  random?: () => number;
  now?: () => number;
}

/** An HTTP failure from the API or storage; status 0 means no response (network down). */
export class UploadHttpError extends Error {
  constructor(
    readonly source: 'api' | 'storage',
    readonly status: number,
    message: string,
  ) {
    super(message);
    this.name = 'UploadHttpError';
  }
}

/** A chunk can't be uploaded and retrying won't help (e.g. the server refused it). */
export class UploadError extends Error {
  constructor(
    readonly idx: number,
    readonly reason: unknown,
  ) {
    super(
      `chunk ${idx} failed to upload: ${reason instanceof Error ? reason.message : String(reason)}`,
    );
    this.name = 'UploadError';
  }
}

/** First retry delay; doubles per attempt up to `MAX_BACKOFF_MS`. */
export const BASE_BACKOFF_MS = 1_000;
export const MAX_BACKOFF_MS = 30_000;
/** Most URLs presigned in one call (the API's `?count=` limit). */
export const PRESIGN_BATCH = 10;
/** Presigned URLs live 5 minutes; stop using a cached one a minute early. */
const URL_REUSE_MS = 4 * 60_000;
/** A PUT that hasn't finished by then is abandoned and retried. */
export const PUT_TIMEOUT_MS = 120_000;

/**
 * Whether trying again can succeed: no response, timeouts, throttling and server errors can;
 * so can a storage `403` (the presigned URL expired: the retry presigns afresh). Other API
 * `4xx`s are the server's considered answer.
 */
export function isRetryable(error: unknown): boolean {
  if (error instanceof UploadError) {
    return false;
  }
  if (error instanceof UploadHttpError) {
    const { status, source } = error;
    return (
      status === 0 ||
      status === 408 ||
      status === 429 ||
      status >= 500 ||
      (source === 'storage' && status === 403)
    );
  }
  // fetch's TypeError, aborts and timeouts: the network, not the server.
  return true;
}

/** `PUT`s the bytes with `fetch`, giving up after `PUT_TIMEOUT_MS`. */
export const fetchPut: PutChunk = async (url, blob) => {
  let response: Response;
  try {
    response = await fetch(url, {
      method: 'PUT',
      body: blob,
      signal: AbortSignal.timeout(PUT_TIMEOUT_MS),
    });
  } catch (error) {
    throw new UploadHttpError('storage', 0, error instanceof Error ? error.message : 'network');
  }
  if (!response.ok) {
    throw new UploadHttpError('storage', response.status, `storage answered ${response.status}`);
  }
};

/** `navigator.onLine` and the `online` event, where they exist. */
export const browserNetwork: NetworkPort = {
  isOnline: () => globalThis.navigator?.onLine ?? true,
  whenOnline: () =>
    new Promise((resolve) => {
      if (globalThis.navigator?.onLine ?? true) {
        resolve();
        return;
      }
      globalThis.addEventListener('online', () => resolve(), { once: true });
    }),
};

/** SHA-256 of the blob's bytes via SubtleCrypto, as lowercase hex. */
export async function sha256Hex(blob: Blob, subtle: SubtleCrypto = crypto.subtle): Promise<string> {
  const digest = new Uint8Array(await subtle.digest('SHA-256', await blob.arrayBuffer()));
  return Array.from(digest, (byte) => byte.toString(16).padStart(2, '0')).join('');
}

/**
 * Streams a take's chunks to storage while it records: for each enqueued index, read the chunk
 * from the local store, hash it, presign (batched when there is a backlog), `PUT` it straight to
 * storage (media never passes through the API), then ack its size and hash. One chunk at a
 * time, in order. Network trouble never loses a chunk: it stays in the store, the uploader
 * waits while offline and backs off (with jitter) on transient failures, and `resume()` picks
 * up from what the server already has.
 */
export class Uploader {
  private readonly api: UploadApi;
  private readonly store: ChunkStore;
  private readonly takeId: string;
  private readonly storeTakeId: string;
  private readonly put: PutChunk;
  private readonly digest: DigestChunk;
  private readonly network: NetworkPort;
  private readonly delay: (ms: number) => Promise<void>;
  private readonly random: () => number;
  private readonly now: () => number;

  private readonly pending: number[] = [];
  private readonly done = new Set<number>();
  private readonly urls = new Map<number, { url: string; fetchedAt: number }>();
  private readonly progress: BehaviorSubject<UploadProgress>;
  private queued = 0;
  private state: UploaderState = 'idle';
  private running: Promise<void> | null = null;
  private failure: UploadError | null = null;
  private cancelled = false;

  constructor(options: UploaderOptions) {
    this.api = options.api;
    this.store = options.store;
    this.takeId = options.takeId;
    this.storeTakeId = options.storeTakeId ?? options.takeId;
    this.put = options.put ?? fetchPut;
    this.digest = options.digest ?? ((blob) => sha256Hex(blob));
    this.network = options.network ?? browserNetwork;
    this.delay = options.delay ?? ((ms) => new Promise((resolve) => setTimeout(resolve, ms)));
    this.random = options.random ?? Math.random;
    this.now = options.now ?? Date.now;
    this.progress = new BehaviorSubject<UploadProgress>({ queued: 0, uploaded: 0, state: 'idle' });
  }

  get progress$(): Observable<UploadProgress> {
    return this.progress.asObservable();
  }

  /** Indexes uploaded and acknowledged so far. */
  get uploaded(): ReadonlySet<number> {
    return this.done;
  }

  /** Queues chunk `idx` (already in the store). Queuing an index twice uploads it once. */
  enqueue(idx: number): void {
    if (this.cancelled || this.done.has(idx) || this.pending.includes(idx)) {
      return;
    }
    this.pending.push(idx);
    this.queued += 1;
    this.emit();
    this.start();
  }

  /**
   * Continues a take uploaded earlier (after a reload, a crash or a reconnect): asks the server
   * which chunks it has, skips those whose recorded hash matches the local copy, and queues
   * every other chunk in the store. A server chunk that differs from the local one is a
   * failure: the server keeps the first ack, so the take can't be completed from this device.
   * With `upTo`, only chunks below that index are queued.
   */
  async resume(upTo = Number.POSITIVE_INFINITY): Promise<void> {
    const status = await this.withRetry(() => this.api.status(this.takeId));
    const local = (await this.store.indexes(this.storeTakeId)).filter((idx) => idx < upTo);
    const localSet = new Set(local);
    for (const chunk of status.chunks) {
      if (localSet.has(chunk.idx)) {
        const blob = await this.store.get(this.storeTakeId, chunk.idx);
        if (blob && (await this.digest(blob)) !== chunk.sha256) {
          this.fail(new UploadError(chunk.idx, new Error('the server has a different copy')));
          return;
        }
      }
      if (!this.done.has(chunk.idx)) {
        this.done.add(chunk.idx);
        this.queued += 1;
      }
    }
    this.emit();
    if (!status.finalized) {
      local.filter((idx) => !this.done.has(idx)).forEach((idx) => this.enqueue(idx));
    }
  }

  /**
   * Finishes the take once recording has stopped (§10 Record step 8): waits for the queue to
   * drain, finalizes with the chunk count and pause-excluding duration (retrying transient
   * failures like any upload step), then deletes the take from the device, since the server
   * now has every chunk. If the server reports chunks missing, uploads them from the store
   * (`resume()`) and finalizes once more.
   */
  async finalize(chunkCount: number, durationMs: number): Promise<FinalizedTake> {
    for (let attempt = 0; ; attempt += 1) {
      await this.drained();
      this.setState('finalizing');
      try {
        const finalized = await this.withRetry(() =>
          this.api.finalize(this.takeId, chunkCount, durationMs),
        );
        await this.store.deleteTake(this.storeTakeId);
        this.setState('finalized');
        return finalized;
      } catch (error) {
        const missing = error instanceof UploadHttpError && error.status === 422;
        if (!missing || attempt > 0) {
          const failure = new UploadError(chunkCount - 1, error);
          this.fail(failure);
          throw failure;
        }
        await this.resume(chunkCount);
      }
    }
  }

  /** Resolves once every queued chunk is uploaded; rejects with the first `UploadError`. */
  async drained(): Promise<void> {
    while (this.running) {
      await this.running;
    }
    if (this.failure) {
      throw this.failure;
    }
  }

  /**
   * Abandons the take's upload (the take was discarded): nothing more is queued or sent. A chunk
   * already in flight finishes or fails on its own; its outcome no longer matters. Does not wait.
   */
  cancel(): void {
    this.cancelled = true;
    this.pending.length = 0;
  }

  private start(): void {
    if (this.failure || this.cancelled) {
      return;
    }
    this.running ??= this.drain().finally(() => {
      this.running = null;
      if (!this.failure && this.state !== 'finalizing') {
        this.setState('idle');
      }
    });
  }

  private async drain(): Promise<void> {
    while (this.pending.length > 0 && !this.failure && !this.cancelled) {
      const idx = this.pending[0];
      this.setState('uploading');
      try {
        await this.withRetry(() => this.upload(idx));
        if (this.cancelled) {
          return;
        }
        this.pending.shift();
        this.done.add(idx);
        this.urls.delete(idx);
        this.emit();
      } catch (error) {
        this.fail(error instanceof UploadError ? error : new UploadError(idx, error));
      }
    }
  }

  /** Runs `step` until it succeeds or fails for good, waiting out offline spells and backoff. */
  private async withRetry<T>(step: () => Promise<T>): Promise<T> {
    let attempt = 0;
    for (;;) {
      if (!this.network.isOnline()) {
        this.setState('offline');
        await this.network.whenOnline();
        this.setState('uploading');
      }
      try {
        return await step();
      } catch (error) {
        if (!isRetryable(error)) {
          throw error;
        }
        if (!this.network.isOnline()) {
          // Lost the connection mid-step: wait for it, don't count it as an attempt.
          continue;
        }
        attempt += 1;
        this.setState('retrying');
        await this.delay(this.backoff(attempt));
        this.setState('uploading');
      }
    }
  }

  /** Exponential backoff with "equal jitter": half fixed, half random. */
  private backoff(attempt: number): number {
    const ceiling = Math.min(MAX_BACKOFF_MS, BASE_BACKOFF_MS * 2 ** (attempt - 1));
    return ceiling / 2 + this.random() * (ceiling / 2);
  }

  private async upload(idx: number): Promise<void> {
    const blob = await this.store.get(this.storeTakeId, idx);
    if (!blob) {
      throw new UploadError(idx, new Error('chunk is not in the local store'));
    }
    const sha256 = await this.digest(blob);
    const url = await this.urlFor(idx);
    try {
      await this.put(url, blob);
    } catch (error) {
      // Whatever went wrong, the next attempt presigns afresh (the URL may have expired).
      this.urls.delete(idx);
      throw error;
    }
    await this.api.ack(this.takeId, idx, blob.size, sha256);
  }

  /** A fresh-enough URL for `idx`, presigning a batch for the backlog when needed. */
  private async urlFor(idx: number): Promise<string> {
    const cached = this.urls.get(idx);
    if (cached && this.now() - cached.fetchedAt < URL_REUSE_MS) {
      return cached.url;
    }
    // A run of consecutive pending indexes starting at idx shares one presign call.
    let count = 1;
    while (count < PRESIGN_BATCH && this.pending.includes(idx + count)) {
      count += 1;
    }
    const fetchedAt = this.now();
    for (const presigned of await this.api.presign(this.takeId, idx, count)) {
      this.urls.set(presigned.idx, { url: presigned.url, fetchedAt });
    }
    const fresh = this.urls.get(idx);
    if (!fresh) {
      throw new UploadError(idx, new Error('no upload URL returned'));
    }
    return fresh.url;
  }

  private fail(error: UploadError): void {
    this.failure = error;
    this.setState('failed');
  }

  private setState(state: UploaderState): void {
    if (this.state !== state) {
      this.state = state;
      this.emit();
    }
  }

  private emit(): void {
    this.progress.next({ queued: this.queued, uploaded: this.done.size, state: this.state });
  }
}
