import { ChunkStore, TakeMeta } from './chunk-store';
import {
  PresignedUrl,
  UploadApi,
  UploadError,
  UploadProgress,
  Uploader,
  sha256Hex,
} from './uploader';

class MemoryStore implements ChunkStore {
  readonly backend = 'indexeddb' as const;
  readonly chunks = new Map<string, Blob>();
  async put(takeId: string, index: number, blob: Blob): Promise<void> {
    this.chunks.set(`${takeId}/${index}`, blob);
  }
  async get(takeId: string, index: number): Promise<Blob | null> {
    return this.chunks.get(`${takeId}/${index}`) ?? null;
  }
  async indexes(): Promise<number[]> {
    return [];
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
  readonly metas: TakeMeta[] = [];
  async putMeta(meta: TakeMeta): Promise<void> {
    this.metas.push(meta);
  }
  async getMeta(): Promise<TakeMeta | null> {
    return null;
  }
}

/** Records every call in order; storage keeps what was PUT. */
class FakeApi implements UploadApi {
  readonly calls: string[] = [];
  readonly stored = new Map<string, Blob>();
  readonly acked = new Map<number, { size: number; sha256: string }>();
  failAckAt: number | null = null;

  async presign(takeId: string, idx: number, count: number): Promise<PresignedUrl[]> {
    this.calls.push(`presign ${idx}x${count}`);
    return [{ idx, url: `https://store.test/${takeId}/${idx}` }];
  }

  async ack(_takeId: string, idx: number, sizeBytes: number, sha256: string): Promise<void> {
    this.calls.push(`ack ${idx}`);
    if (idx === this.failAckAt) {
      throw new Error('server said no');
    }
    this.acked.set(idx, { size: sizeBytes, sha256 });
  }

  readonly put = async (url: string, blob: Blob): Promise<void> => {
    this.calls.push(`put ${url.split('/').pop()}`);
    this.stored.set(url, blob);
  };
}

const TAKE = '01a0e73b-b633-7250-ac1e-b1313edaa69e';

function setup() {
  const store = new MemoryStore();
  const api = new FakeApi();
  const uploader = new Uploader({ api, store, takeId: TAKE, put: api.put });
  return { store, api, uploader };
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

describe('Uploader', () => {
  it('presigns, PUTs, then acks each chunk with its size and hash, in order', async () => {
    const { store, api, uploader } = setup();
    await store.put(TAKE, 0, new Blob(['abc']));
    await store.put(TAKE, 1, new Blob(['hello']));

    uploader.enqueue(0);
    uploader.enqueue(1);
    await uploader.drained();

    expect(api.calls).toEqual(['presign 0x1', 'put 0', 'ack 0', 'presign 1x1', 'put 1', 'ack 1']);
    expect(api.acked.get(0)).toEqual({
      size: 3,
      sha256: 'ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad',
    });
    expect(api.acked.get(1)?.size).toBe(5);
    expect(await api.stored.get(`https://store.test/${TAKE}/1`)?.text()).toBe('hello');
    expect([...uploader.uploaded]).toEqual([0, 1]);
  });

  it('reports progress as chunks are queued and uploaded', async () => {
    const { store, uploader } = setup();
    const seen: UploadProgress[] = [];
    uploader.progress$.subscribe((progress) => seen.push(progress));
    await store.put(TAKE, 0, new Blob(['a']));
    await store.put(TAKE, 1, new Blob(['b']));

    uploader.enqueue(0);
    uploader.enqueue(1);
    await uploader.drained();

    expect(seen.at(-1)).toEqual({ queued: 2, uploaded: 2 });
    expect(seen).toContainEqual({ queued: 2, uploaded: 1 });
  });

  it('uploads an index once even if it is enqueued twice', async () => {
    const { store, api, uploader } = setup();
    await store.put(TAKE, 0, new Blob(['a']));
    uploader.enqueue(0);
    uploader.enqueue(0);
    await uploader.drained();
    uploader.enqueue(0);
    await uploader.drained();
    expect(api.calls.filter((call) => call.startsWith('put'))).toEqual(['put 0']);
  });

  it('keeps uploading chunks enqueued while it drains', async () => {
    const { store, uploader } = setup();
    for (const idx of [0, 1, 2]) {
      await store.put(TAKE, idx, new Blob([`chunk ${idx}`]));
    }
    uploader.enqueue(0);
    const drained = uploader.drained();
    uploader.enqueue(1);
    uploader.enqueue(2);
    await drained;
    expect([...uploader.uploaded]).toEqual([0, 1, 2]);
  });

  it('stops at a failed chunk and reports it', async () => {
    const { store, api, uploader } = setup();
    for (const idx of [0, 1, 2]) {
      await store.put(TAKE, idx, new Blob(['x']));
    }
    api.failAckAt = 1;
    [0, 1, 2].forEach((idx) => uploader.enqueue(idx));

    const error = await uploader.drained().catch((e: unknown) => e);
    expect(error).toBeInstanceOf(UploadError);
    expect((error as UploadError).idx).toBe(1);
    expect([...uploader.uploaded]).toEqual([0]);
    expect(api.calls).not.toContain('presign 2x1');
  });

  it('fails a chunk that is not in the local store', async () => {
    const { uploader } = setup();
    uploader.enqueue(4);
    const error = await uploader.drained().catch((e: unknown) => e);
    expect((error as UploadError).idx).toBe(4);
    expect((error as UploadError).message).toContain('not in the local store');
  });
});
