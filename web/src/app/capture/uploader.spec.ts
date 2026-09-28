import { ChunkStore, TakeMeta } from './chunk-store';
import {
  MAX_BACKOFF_MS,
  NetworkPort,
  PresignedUrl,
  UploadApi,
  UploadError,
  UploadHttpError,
  UploadProgress,
  UploadStatus,
  Uploader,
  isRetryable,
  sha256Hex,
} from './uploader';

class MemoryStore implements ChunkStore {
  readonly backend = 'indexeddb' as const;
  readonly chunks = new Map<string, Blob>();
  readonly metas: TakeMeta[] = [];
  async put(takeId: string, index: number, blob: Blob): Promise<void> {
    this.chunks.set(`${takeId}/${index}`, blob);
  }
  async get(takeId: string, index: number): Promise<Blob | null> {
    return this.chunks.get(`${takeId}/${index}`) ?? null;
  }
  async indexes(takeId: string): Promise<number[]> {
    return [...this.chunks.keys()]
      .filter((key) => key.startsWith(`${takeId}/`))
      .map((key) => Number(key.split('/')[1]))
      .sort((a, b) => a - b);
  }
  async takes(): Promise<string[]> {
    return [];
  }
  async deleteTake(takeId: string): Promise<void> {
    for (const key of [...this.chunks.keys()]) {
      if (key.startsWith(`${takeId}/`)) {
        this.chunks.delete(key);
      }
    }
  }
  async putMeta(meta: TakeMeta): Promise<void> {
    this.metas.push(meta);
  }
  async getMeta(): Promise<TakeMeta | null> {
    return null;
  }
}

/** Records every call in order. Scripted failures are consumed one per call. */
class FakeApi implements UploadApi {
  readonly calls: string[] = [];
  readonly stored = new Map<string, Blob>();
  readonly acked = new Map<number, { size: number; sha256: string }>();
  readonly putFailures: unknown[] = [];
  readonly ackFailures: unknown[] = [];
  serverStatus: UploadStatus = { finalized: false, chunks: [] };
  urlVersion = 0;

  async presign(takeId: string, idx: number, count: number): Promise<PresignedUrl[]> {
    this.calls.push(`presign ${idx}x${count}`);
    this.urlVersion += 1;
    return Array.from({ length: count }, (_, i) => ({
      idx: idx + i,
      url: `https://store.test/${takeId}/${idx + i}?v=${this.urlVersion}`,
    }));
  }

  async ack(_takeId: string, idx: number, sizeBytes: number, sha256: string): Promise<void> {
    this.calls.push(`ack ${idx}`);
    const failure = this.ackFailures.shift();
    if (failure) {
      throw failure;
    }
    this.acked.set(idx, { size: sizeBytes, sha256 });
  }

  async status(): Promise<UploadStatus> {
    this.calls.push('status');
    return this.serverStatus;
  }

  readonly finalizeFailures: unknown[] = [];
  async finalize(
    _takeId: string,
    chunkCount: number,
    durationMs: number,
  ): Promise<{ recording_id: string }> {
    this.calls.push(`finalize ${chunkCount} ${durationMs}`);
    const failure = this.finalizeFailures.shift();
    if (failure) {
      throw failure;
    }
    return { recording_id: 'rec-1' };
  }

  readonly put = async (url: string, blob: Blob): Promise<void> => {
    const idx = url.split('/').pop()?.split('?')[0];
    this.calls.push(`put ${idx}`);
    const failure = this.putFailures.shift();
    if (failure) {
      throw failure;
    }
    this.stored.set(url, blob);
  };
}

/** A network whose connectivity the test flips. */
class FakeNetwork implements NetworkPort {
  online = true;
  private waiters: (() => void)[] = [];
  isOnline(): boolean {
    return this.online;
  }
  whenOnline(): Promise<void> {
    return this.online ? Promise.resolve() : new Promise((resolve) => this.waiters.push(resolve));
  }
  goOnline(): void {
    this.online = true;
    this.waiters.splice(0).forEach((resolve) => resolve());
  }
}

const TAKE = '01a0e73b-b633-7250-ac1e-b1313edaa69e';
const flush = () => new Promise((resolve) => setTimeout(resolve, 0));

function setup() {
  const store = new MemoryStore();
  const api = new FakeApi();
  const network = new FakeNetwork();
  const delays: number[] = [];
  const uploader = new Uploader({
    api,
    store,
    takeId: TAKE,
    put: api.put,
    network,
    delay: async (ms) => {
      delays.push(ms);
    },
    random: () => 0.5,
  });
  return { store, api, network, delays, uploader };
}

async function fill(store: MemoryStore, indexes: number[]): Promise<void> {
  for (const idx of indexes) {
    await store.put(TAKE, idx, new Blob([`chunk ${idx}`]));
  }
}

describe('sha256Hex', () => {
  it('matches the standard test vectors', async () => {
    expect(await sha256Hex(new Blob([]))).toBe(
      'e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855',
    );
    expect(await sha256Hex(new Blob(['abc']))).toBe(
      'ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad',
    );
  });
});

describe('isRetryable', () => {
  it('retries the network, throttling, server errors and expired storage URLs', () => {
    expect(isRetryable(new TypeError('Failed to fetch'))).toBe(true);
    for (const status of [0, 408, 429, 500, 502, 503]) {
      expect(isRetryable(new UploadHttpError('api', status, ''))).toBe(true);
    }
    expect(isRetryable(new UploadHttpError('storage', 403, ''))).toBe(true);
  });

  it('does not retry the API refusing', () => {
    for (const status of [400, 401, 403, 404, 409, 422]) {
      expect(isRetryable(new UploadHttpError('api', status, ''))).toBe(false);
    }
    expect(isRetryable(new UploadError(0, 'x'))).toBe(false);
  });
});

describe('Uploader', () => {
  it('presigns, PUTs, then acks each chunk with its size and hash, in order', async () => {
    const { store, api, uploader } = setup();
    await store.put(TAKE, 0, new Blob(['abc']));
    uploader.enqueue(0);
    await uploader.drained();
    await store.put(TAKE, 1, new Blob(['hello']));
    uploader.enqueue(1);
    await uploader.drained();

    expect(api.calls).toEqual(['presign 0x1', 'put 0', 'ack 0', 'presign 1x1', 'put 1', 'ack 1']);
    expect(api.acked.get(0)).toEqual({
      size: 3,
      sha256: 'ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad',
    });
    expect(api.acked.get(1)?.size).toBe(5);
    expect([...uploader.uploaded]).toEqual([0, 1]);
  });

  it('presigns a backlog in one batch', async () => {
    const { store, api, uploader } = setup();
    await fill(store, [0, 1, 2, 3]);
    [0, 1, 2, 3].forEach((idx) => uploader.enqueue(idx));
    await uploader.drained();
    expect(api.calls.filter((call) => call.startsWith('presign'))).toEqual(['presign 0x4']);
    expect(api.acked.size).toBe(4);
  });

  it('reports progress and state', async () => {
    const { store, uploader } = setup();
    const seen: UploadProgress[] = [];
    uploader.progress$.subscribe((progress) => seen.push(progress));
    await fill(store, [0, 1]);
    uploader.enqueue(0);
    uploader.enqueue(1);
    await uploader.drained();
    expect(seen).toContainEqual({ queued: 2, uploaded: 1, state: 'uploading' });
    expect(seen.at(-1)).toEqual({ queued: 2, uploaded: 2, state: 'idle' });
  });

  it('uploads an index once even if it is enqueued twice', async () => {
    const { store, api, uploader } = setup();
    await fill(store, [0]);
    uploader.enqueue(0);
    uploader.enqueue(0);
    await uploader.drained();
    uploader.enqueue(0);
    await uploader.drained();
    expect(api.calls.filter((call) => call.startsWith('put'))).toEqual(['put 0']);
  });

  it('retries transient failures with growing, jittered backoff', async () => {
    const { store, api, delays, uploader } = setup();
    await fill(store, [0]);
    api.putFailures.push(
      new UploadHttpError('storage', 503, 'busy'),
      new TypeError('Failed to fetch'),
    );
    api.ackFailures.push(new UploadHttpError('api', 502, 'bad gateway'));
    uploader.enqueue(0);
    await uploader.drained();

    expect(api.acked.has(0)).toBe(true);
    // random() = 0.5 → 3/4 of each ceiling: 1 s, 2 s, 4 s.
    expect(delays).toEqual([750, 1500, 3000]);
    // Every failed PUT presigns afresh.
    expect(api.calls.filter((call) => call.startsWith('presign'))).toHaveLength(3);
  });

  it('caps the backoff', async () => {
    const { store, api, delays, uploader } = setup();
    await fill(store, [0]);
    for (let i = 0; i < 10; i += 1) {
      api.putFailures.push(new UploadHttpError('storage', 500, 'down'));
    }
    uploader.enqueue(0);
    await uploader.drained();
    expect(Math.max(...delays)).toBe(MAX_BACKOFF_MS * 0.75);
  });

  it('waits while offline and continues when the network returns', async () => {
    const { store, api, network, delays, uploader } = setup();
    const states: string[] = [];
    uploader.progress$.subscribe((progress) => states.push(progress.state));
    await fill(store, [0, 1]);
    network.online = false;
    uploader.enqueue(0);
    uploader.enqueue(1);
    await flush();
    expect(api.calls).toEqual([]);
    expect(states.at(-1)).toBe('offline');

    network.goOnline();
    await uploader.drained();
    expect(api.acked.size).toBe(2);
    expect(delays).toEqual([]);
  });

  it('does not count a connection lost mid-upload as a failed attempt', async () => {
    const { store, api, network, delays } = setup();
    await fill(store, [0]);
    api.putFailures.push(new TypeError('Failed to fetch'));
    const put = api.put;
    let first = true;
    const uploaderWithDrop = new Uploader({
      api,
      store,
      takeId: TAKE,
      network,
      delay: async (ms) => {
        delays.push(ms);
      },
      put: async (url, blob) => {
        if (first) {
          first = false;
          network.online = false;
          setTimeout(() => network.goOnline(), 0);
        }
        await put(url, blob);
      },
    });
    uploaderWithDrop.enqueue(0);
    await uploaderWithDrop.drained();
    expect(api.acked.has(0)).toBe(true);
    expect(delays).toEqual([]);
  });

  it('stops for good when the server refuses a chunk', async () => {
    const { store, api, uploader } = setup();
    await fill(store, [0, 1, 2]);
    api.ackFailures.push(undefined, new UploadHttpError('api', 409, 'different hash'));
    [0, 1, 2].forEach((idx) => uploader.enqueue(idx));

    const error = await uploader.drained().catch((e: unknown) => e);
    expect(error).toBeInstanceOf(UploadError);
    expect((error as UploadError).idx).toBe(1);
    expect([...uploader.uploaded]).toEqual([0]);
    expect(api.calls).not.toContain('put 2');
  });

  it('fails a chunk that is not in the local store', async () => {
    const { uploader } = setup();
    uploader.enqueue(4);
    const error = await uploader.drained().catch((e: unknown) => e);
    expect((error as UploadError).idx).toBe(4);
    expect((error as UploadError).message).toContain('not in the local store');
  });

  describe('resume', () => {
    it('skips what the server already has and uploads the rest', async () => {
      const { store, api, uploader } = setup();
      await fill(store, [0, 1, 2, 3]);
      api.serverStatus = {
        finalized: false,
        chunks: [
          { idx: 0, size_bytes: 7, sha256: await sha256Hex(new Blob(['chunk 0'])) },
          { idx: 1, size_bytes: 7, sha256: await sha256Hex(new Blob(['chunk 1'])) },
        ],
      };
      await uploader.resume();
      await uploader.drained();

      expect(api.calls.filter((call) => call.startsWith('put'))).toEqual(['put 2', 'put 3']);
      expect([...uploader.uploaded].sort()).toEqual([0, 1, 2, 3]);
    });

    it('fails when the server holds a different copy of a chunk', async () => {
      const { store, api, uploader } = setup();
      await fill(store, [0]);
      api.serverStatus = {
        finalized: false,
        chunks: [{ idx: 0, size_bytes: 7, sha256: 'ab'.repeat(32) }],
      };
      await uploader.resume();
      const error = await uploader.drained().catch((e: unknown) => e);
      expect((error as UploadError).message).toContain('different copy');
    });

    it('uploads nothing for a finalized take', async () => {
      const { store, api, uploader } = setup();
      await fill(store, [0, 1]);
      api.serverStatus = { finalized: true, chunks: [] };
      await uploader.resume();
      await uploader.drained();
      expect(api.calls).toEqual(['status']);
    });

    it('retries the status call while the network is down', async () => {
      const { store, api, network, uploader } = setup();
      await fill(store, [0]);
      network.online = false;
      const resumed = uploader.resume();
      await flush();
      expect(api.calls).toEqual([]);
      network.goOnline();
      await resumed;
      await uploader.drained();
      expect(api.acked.has(0)).toBe(true);
    });
  });

  describe('finalize', () => {
    it('drains, finalizes, then clears the take from the device', async () => {
      const { store, api, uploader } = setup();
      await fill(store, [0, 1, 2]);
      [0, 1, 2].forEach((idx) => uploader.enqueue(idx));
      const finalized = await uploader.finalize(3, 6000);

      expect(finalized).toEqual({ recording_id: 'rec-1' });
      expect(api.calls.at(-1)).toBe('finalize 3 6000');
      expect(api.calls.indexOf('ack 2')).toBeLessThan(api.calls.indexOf('finalize 3 6000'));
      expect(await store.indexes(TAKE)).toEqual([]);
    });

    it('retries a transient failure', async () => {
      const { store, api, delays, uploader } = setup();
      await fill(store, [0]);
      uploader.enqueue(0);
      api.finalizeFailures.push(new UploadHttpError('api', 503, 'busy'));
      await uploader.finalize(1, 2000);
      expect(delays).toEqual([750]);
      expect(api.calls.filter((call) => call.startsWith('finalize'))).toHaveLength(2);
    });

    it('uploads missing chunks the server reports, then finalizes again', async () => {
      const { store, api, uploader } = setup();
      await fill(store, [0, 1]);
      // Only chunk 0 was ever enqueued; the server says 1 is missing.
      uploader.enqueue(0);
      api.finalizeFailures.push(new UploadHttpError('api', 422, 'missing'));
      api.serverStatus = {
        finalized: false,
        chunks: [{ idx: 0, size_bytes: 7, sha256: await sha256Hex(new Blob(['chunk 0'])) }],
      };
      await uploader.finalize(2, 4000);
      expect(api.acked.has(1)).toBe(true);
      expect(api.calls.filter((call) => call.startsWith('finalize'))).toHaveLength(2);
    });

    it('keeps the take on the device when finalize is refused', async () => {
      const { store, api, uploader } = setup();
      await fill(store, [0]);
      uploader.enqueue(0);
      api.finalizeFailures.push(new UploadHttpError('api', 409, 'closed'));
      await expect(uploader.finalize(1, 2000)).rejects.toBeInstanceOf(UploadError);
      expect(await store.indexes(TAKE)).toEqual([0]);
    });
  });
});
