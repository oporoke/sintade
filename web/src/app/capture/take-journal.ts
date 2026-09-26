import { Observable, Subject } from 'rxjs';

import { ChunkRecorder } from './chunk-recorder';
import { ChunkStore, TakeMeta } from './chunk-store';

/** The slice of the Web Locks API used to tell live takes from orphaned ones. */
export type LocksPort = Pick<LockManager, 'request' | 'query'>;

export interface PersistOptions {
  takeId: string;
  mimeType: string;
  startedAt?: number;
  /** Defaults to `navigator.locks`; `null` records without a lock. */
  locks?: LocksPort | null;
}

export interface PersistedTake {
  /** Emits the journal after each chunk is durably stored. */
  readonly persisted$: Observable<TakeMeta>;
  /** Resolves with the final journal once the recording stopped and every chunk is stored. */
  readonly done: Promise<TakeMeta>;
}

export interface OrphanTake {
  takeId: string;
  /** Epoch ms, or null when the journal is missing. */
  startedAt: number | null;
  /** Recorded time up to the last stored chunk, or null when the journal is missing. */
  durationMs: number | null;
  chunkCount: number;
  mimeType: string;
}

export interface AssembledTake {
  blob: Blob;
  /** Chunks concatenated: the contiguous run from index 0. */
  chunkCount: number;
  /** Indexes after the first gap, which can't be appended to a playable file. */
  unusable: number[];
}

const lockName = (takeId: string) => `sintade-take-${takeId}`;

/**
 * Writes every chunk of a recording to the store before anything else happens to it (§10
 * Record step 7, US-12), and journals the take after each chunk so that, if the tab dies, the
 * recovery dialog knows when it started and how long it is. While recording, the tab holds a
 * Web Lock for the take, which is how `listOrphans` tells a live take from an abandoned one.
 * Call before `recorder.start()`.
 */
export async function persistTake(
  recorder: ChunkRecorder,
  store: ChunkStore,
  options: PersistOptions,
): Promise<PersistedTake> {
  const locks =
    options.locks === undefined ? globalThis.navigator?.locks : (options.locks ?? undefined);
  const release = await acquireLock(locks, lockName(options.takeId));
  const meta: TakeMeta = {
    takeId: options.takeId,
    startedAt: options.startedAt ?? Date.now(),
    mimeType: options.mimeType,
    chunkCount: 0,
    durationMs: 0,
  };
  await store.putMeta(meta);

  const persisted = new Subject<TakeMeta>();
  let queue = Promise.resolve();
  const done = new Promise<TakeMeta>((resolve, reject) => {
    recorder.chunks$.subscribe({
      next: ({ index, blob }) => {
        // Read the timer now: it's how much recording this chunk completes.
        const durationMs = recorder.elapsedMs();
        queue = queue.then(async () => {
          await store.put(options.takeId, index, blob);
          meta.chunkCount = Math.max(meta.chunkCount, index + 1);
          meta.durationMs = Math.max(meta.durationMs, durationMs);
          await store.putMeta({ ...meta });
          persisted.next({ ...meta });
        });
        queue.catch(() => undefined);
      },
      error: (error: unknown) => {
        release();
        persisted.error(error);
        reject(error);
      },
      complete: () => {
        queue.then(
          () => {
            release();
            persisted.complete();
            resolve({ ...meta });
          },
          (error: unknown) => {
            release();
            persisted.error(error);
            reject(error);
          },
        );
      },
    });
  });
  done.catch(() => undefined);
  return { persisted$: persisted.asObservable(), done };
}

/** Stored takes whose recording tab is gone (no lock held), oldest first (§10 Recover). */
export async function listOrphans(
  store: ChunkStore,
  locks: LocksPort | undefined = globalThis.navigator?.locks,
): Promise<OrphanTake[]> {
  const held = new Set(
    ((await locks?.query())?.held ?? []).map((lock) => lock.name).filter(Boolean),
  );
  const orphans: OrphanTake[] = [];
  for (const takeId of await store.takes()) {
    if (held.has(lockName(takeId))) {
      continue;
    }
    const [meta, indexes] = await Promise.all([store.getMeta(takeId), store.indexes(takeId)]);
    orphans.push({
      takeId,
      startedAt: meta?.startedAt ?? null,
      durationMs: meta?.durationMs ?? null,
      chunkCount: indexes.length,
      mimeType: meta?.mimeType ?? '',
    });
  }
  return orphans.sort((a, b) => (a.startedAt ?? 0) - (b.startedAt ?? 0));
}

/** Concatenates a stored take's chunks in index order into one playable file. */
export async function assembleTake(store: ChunkStore, takeId: string): Promise<AssembledTake> {
  const [meta, indexes] = await Promise.all([store.getMeta(takeId), store.indexes(takeId)]);
  const parts: Blob[] = [];
  let next = 0;
  while (indexes.includes(next)) {
    const chunk = await store.get(takeId, next);
    if (!chunk) {
      break;
    }
    parts.push(chunk);
    next += 1;
  }
  return {
    blob: new Blob(parts, { type: meta?.mimeType ?? parts[0]?.type ?? '' }),
    chunkCount: parts.length,
    unusable: indexes.filter((index) => index >= next),
  };
}

/** Resolves once the lock is held; the returned function releases it. No-op without Web Locks. */
async function acquireLock(locks: LocksPort | undefined, name: string): Promise<() => void> {
  if (!locks) {
    return () => undefined;
  }
  let release: () => void = () => undefined;
  const released = new Promise<void>((resolve) => (release = resolve));
  await new Promise<void>((acquired) => {
    void locks.request(name, { mode: 'exclusive' }, () => {
      acquired();
      return released;
    });
  });
  return release;
}
