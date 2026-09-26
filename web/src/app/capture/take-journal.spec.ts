import { Subject } from 'rxjs';

import { Chunk, ChunkRecorder } from './chunk-recorder';
import { ChunkStore, TakeMeta } from './chunk-store';
import { LocksPort, assembleTake, listOrphans, persistTake } from './take-journal';

class MemoryStore implements ChunkStore {
  readonly backend = 'indexeddb' as const;
  readonly chunks = new Map<string, Map<number, Blob>>();
  readonly metas = new Map<string, TakeMeta>();
  async put(takeId: string, index: number, blob: Blob) {
    const take = this.chunks.get(takeId) ?? new Map<number, Blob>();
    take.set(index, blob);
    this.chunks.set(takeId, take);
  }
  async get(takeId: string, index: number) {
    return this.chunks.get(takeId)?.get(index) ?? null;
  }
  async indexes(takeId: string) {
    return [...(this.chunks.get(takeId)?.keys() ?? [])].sort((a, b) => a - b);
  }
  async takes() {
    return [...this.chunks.keys()].filter((id) => (this.chunks.get(id)?.size ?? 0) > 0).sort();
  }
  async deleteTake(takeId: string) {
    this.chunks.delete(takeId);
    this.metas.delete(takeId);
  }
  async putMeta(meta: TakeMeta) {
    this.metas.set(meta.takeId, { ...meta });
  }
  async getMeta(takeId: string) {
    return this.metas.get(takeId) ?? null;
  }
}

/** Minimal Web Locks: grants immediately, tracks what's held. */
class FakeLocks {
  readonly held = new Set<string>();
  request = vi.fn(
    (name: string, _options: LockOptions, callback: () => Promise<void>): Promise<void> => {
      this.held.add(name);
      return callback().then(() => {
        this.held.delete(name);
      });
    },
  );
  query = vi.fn(async () => ({
    held: [...this.held].map((name) => ({ name, mode: 'exclusive' as const })),
    pending: [],
  }));
}

function fakeRecorder() {
  const chunks = new Subject<Chunk>();
  let elapsed = 0;
  const recorder = { chunks$: chunks.asObservable(), elapsedMs: () => elapsed };
  return {
    recorder: recorder as unknown as ChunkRecorder,
    chunks,
    setElapsed: (ms: number) => (elapsed = ms),
  };
}

const TAKE = '0190a1b2-c3d4-7e5f-8a9b-0c1d2e3f4a5b';
const asLocks = (locks: FakeLocks) => locks as unknown as LocksPort;
const flush = () => new Promise((resolve) => setTimeout(resolve, 0));

describe('persistTake', () => {
  it('stores each chunk, then journals count and pause-excluding duration', async () => {
    const store = new MemoryStore();
    const locks = new FakeLocks();
    const { recorder, chunks, setElapsed } = fakeRecorder();
    const take = await persistTake(recorder, store, {
      takeId: TAKE,
      mimeType: 'video/webm',
      startedAt: 1_700_000_000_000,
      locks: asLocks(locks),
    });
    expect(await store.getMeta(TAKE)).toMatchObject({ chunkCount: 0, durationMs: 0 });

    setElapsed(2000);
    chunks.next({ index: 0, blob: new Blob(['a']) });
    setElapsed(4000);
    chunks.next({ index: 1, blob: new Blob(['b']) });
    await flush();

    expect(await store.indexes(TAKE)).toEqual([0, 1]);
    expect(await store.getMeta(TAKE)).toEqual({
      takeId: TAKE,
      startedAt: 1_700_000_000_000,
      mimeType: 'video/webm',
      chunkCount: 2,
      durationMs: 4000,
    });

    chunks.complete();
    await expect(take.done).resolves.toMatchObject({ chunkCount: 2, durationMs: 4000 });
  });

  it('holds the take lock while recording and releases it when done', async () => {
    const store = new MemoryStore();
    const locks = new FakeLocks();
    const { recorder, chunks } = fakeRecorder();
    const take = await persistTake(recorder, store, {
      takeId: TAKE,
      mimeType: 'video/webm',
      locks: asLocks(locks),
    });
    expect(locks.held.has(`sintade-take-${TAKE}`)).toBe(true);

    chunks.complete();
    await take.done;
    await flush();
    expect(locks.held.size).toBe(0);
  });

  it('writes chunks in order even when writes are slow', async () => {
    const store = new MemoryStore();
    const order: number[] = [];
    const put = store.put.bind(store);
    store.put = async (takeId, index, blob) => {
      await new Promise((resolve) => setTimeout(resolve, index === 0 ? 20 : 0));
      order.push(index);
      return put(takeId, index, blob);
    };
    const { recorder, chunks } = fakeRecorder();
    const take = await persistTake(recorder, store, {
      takeId: TAKE,
      mimeType: 'x',
      locks: null,
    });
    chunks.next({ index: 0, blob: new Blob(['a']) });
    chunks.next({ index: 1, blob: new Blob(['b']) });
    chunks.complete();
    await take.done;
    expect(order).toEqual([0, 1]);
  });
});

describe('listOrphans', () => {
  it('lists stored takes whose lock is not held, oldest first, with journal data', async () => {
    const store = new MemoryStore();
    const locks = new FakeLocks();
    const live = '0190bbbb-0000-7000-8000-000000000001';
    const older = '0190cccc-0000-7000-8000-000000000002';
    for (const id of [TAKE, live, older]) {
      await store.put(id, 0, new Blob(['x']));
    }
    await store.putMeta({
      takeId: TAKE,
      startedAt: 2000,
      mimeType: 'video/webm',
      chunkCount: 1,
      durationMs: 380_000,
    });
    await store.putMeta({
      takeId: older,
      startedAt: 1000,
      mimeType: 'video/webm',
      chunkCount: 1,
      durationMs: 2000,
    });
    locks.held.add(`sintade-take-${live}`);

    const orphans = await listOrphans(store, asLocks(locks));

    expect(orphans.map((orphan) => orphan.takeId)).toEqual([older, TAKE]);
    expect(orphans[1]).toEqual({
      takeId: TAKE,
      startedAt: 2000,
      durationMs: 380_000,
      chunkCount: 1,
      mimeType: 'video/webm',
    });
  });

  it('still lists chunks whose journal is missing, with unknown start and duration', async () => {
    const store = new MemoryStore();
    await store.put(TAKE, 0, new Blob(['x']));
    const [orphan] = await listOrphans(store, undefined);
    expect(orphan).toMatchObject({
      takeId: TAKE,
      startedAt: null,
      durationMs: null,
      chunkCount: 1,
    });
  });
});

describe('assembleTake', () => {
  it('concatenates the contiguous run from index 0 and reports the rest as unusable', async () => {
    const store = new MemoryStore();
    await store.putMeta({
      takeId: TAKE,
      startedAt: 0,
      mimeType: 'video/webm',
      chunkCount: 4,
      durationMs: 0,
    });
    for (const [index, data] of [
      [0, 'ab'],
      [1, 'cd'],
      [3, 'gh'],
    ] as const) {
      await store.put(TAKE, index, new Blob([data]));
    }
    const assembled = await assembleTake(store, TAKE);
    expect(await assembled.blob.text()).toBe('abcd');
    expect(assembled.blob.type).toBe('video/webm');
    expect(assembled.chunkCount).toBe(2);
    expect(assembled.unusable).toEqual([3]);
  });
});
