import { BehaviorSubject, Observable } from 'rxjs';

import { ChunkStore } from './chunk-store';

/** One chunk's presigned upload URL. The URL is a bearer credential: never log it. */
export interface PresignedUrl {
  idx: number;
  url: string;
}

/**
 * The ingest endpoints the uploader needs (upload protocol v1, docs/design.md §9). The app
 * implements it over HTTP; tests and the extension supply their own.
 */
export interface UploadApi {
  presign(takeId: string, idx: number, count: number): Promise<PresignedUrl[]>;
  ack(takeId: string, idx: number, sizeBytes: number, sha256: string): Promise<void>;
}

/** Sends a chunk's bytes to its presigned URL. */
export type PutChunk = (url: string, blob: Blob) => Promise<void>;

/** Hashes a chunk: lowercase hex SHA-256. */
export type DigestChunk = (blob: Blob) => Promise<string>;

export interface UploadProgress {
  /** Chunks enqueued so far (uploaded or not). */
  queued: number;
  /** Chunks uploaded and acknowledged. */
  uploaded: number;
}

export interface UploaderOptions {
  api: UploadApi;
  /** Where the chunks are: every chunk is in the store before it is uploaded (US-12). */
  store: ChunkStore;
  /** The server's take id, also the key the chunks are stored under. */
  takeId: string;
  put?: PutChunk;
  digest?: DigestChunk;
}

/** A chunk failed to upload. Retries arrive on Day 38; for now the queue stops here. */
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

/** `PUT`s the bytes with `fetch`. Storage answers 200 on success. */
export const fetchPut: PutChunk = async (url, blob) => {
  const response = await fetch(url, { method: 'PUT', body: blob });
  if (!response.ok) {
    throw new Error(`storage answered ${response.status}`);
  }
};

/** SHA-256 of the blob's bytes via SubtleCrypto, as lowercase hex. */
export async function sha256Hex(blob: Blob, subtle: SubtleCrypto = crypto.subtle): Promise<string> {
  const digest = new Uint8Array(await subtle.digest('SHA-256', await blob.arrayBuffer()));
  return Array.from(digest, (byte) => byte.toString(16).padStart(2, '0')).join('');
}

/**
 * Streams a take's chunks to storage while it records: for each enqueued index, read the chunk
 * from the local store, hash it, presign, `PUT` it straight to storage (media never passes
 * through the API), then ack its size and hash. One chunk at a time, in the order enqueued.
 */
export class Uploader {
  private readonly api: UploadApi;
  private readonly store: ChunkStore;
  private readonly takeId: string;
  private readonly put: PutChunk;
  private readonly digest: DigestChunk;

  private readonly pending: number[] = [];
  private readonly done = new Set<number>();
  private readonly progress = new BehaviorSubject<UploadProgress>({ queued: 0, uploaded: 0 });
  private queued = 0;
  private running: Promise<void> | null = null;
  private failure: UploadError | null = null;

  constructor(options: UploaderOptions) {
    this.api = options.api;
    this.store = options.store;
    this.takeId = options.takeId;
    this.put = options.put ?? fetchPut;
    this.digest = options.digest ?? ((blob) => sha256Hex(blob));
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
    if (this.done.has(idx) || this.pending.includes(idx)) {
      return;
    }
    this.pending.push(idx);
    this.queued += 1;
    this.emit();
    this.running ??= this.drain().finally(() => {
      this.running = null;
    });
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

  private async drain(): Promise<void> {
    while (this.pending.length > 0 && !this.failure) {
      const idx = this.pending[0];
      try {
        await this.upload(idx);
        this.pending.shift();
        this.done.add(idx);
        this.emit();
      } catch (error) {
        this.failure = error instanceof UploadError ? error : new UploadError(idx, error);
      }
    }
  }

  private async upload(idx: number): Promise<void> {
    const blob = await this.store.get(this.takeId, idx);
    if (!blob) {
      throw new UploadError(idx, new Error('chunk is not in the local store'));
    }
    const sha256 = await this.digest(blob);
    const [presigned] = await this.api.presign(this.takeId, idx, 1);
    if (presigned?.idx !== idx) {
      throw new UploadError(idx, new Error('no upload URL returned'));
    }
    await this.put(presigned.url, blob);
    await this.api.ack(this.takeId, idx, blob.size, sha256);
  }

  private emit(): void {
    this.progress.next({ queued: this.queued, uploaded: this.done.size });
  }
}
