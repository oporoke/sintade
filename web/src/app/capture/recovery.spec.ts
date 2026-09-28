import { ChunkStore, TakeMeta } from './chunk-store';
import { RecoveryApi, uploadRecoveredTake } from './recovery';
import { PresignedUrl, UploadHttpError, UploadStatus, sha256Hex } from './uploader';

class MemoryStore implements ChunkStore {
  readonly backend = 'indexeddb' as const;
  readonly chunks = new Map<string, Blob>();
  readonly metas = new Map<string, TakeMeta>();
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
    this.metas.delete(takeId);
  }
  async putMeta(meta: TakeMeta): Promise<void> {
    this.metas.set(meta.takeId, meta);
  }
  async getMeta(takeId: string): Promise<TakeMeta | null> {
    return this.metas.get(takeId) ?? null;
  }
}

/** A server that keeps acks per take and answers status from them. */
class FakeServer implements RecoveryApi {
  readonly calls: string[] = [];
  readonly acked = new Map<string, Map<number, string>>();
  created = 0;
  finalized: { takeId: string; chunkCount: number; durationMs: number } | null = null;

  async createRecording(): Promise<{ recording_id: string; take_id: string }> {
    this.created += 1;
    this.calls.push('create');
    return { recording_id: `rec-${this.created}`, take_id: `server-take-${this.created}` };
  }
  async presign(takeId: string, idx: number, count: number): Promise<PresignedUrl[]> {
    return Array.from({ length: count }, (_, i) => ({
      idx: idx + i,
      url: `https://s/${takeId}/${idx + i}`,
    }));
  }
  async ack(takeId: string, idx: number, _size: number, sha256: string): Promise<void> {
    this.calls.push(`ack ${takeId} ${idx}`);
    const take = this.acked.get(takeId) ?? new Map<number, string>();
    take.set(idx, sha256);
    this.acked.set(takeId, take);
  }
  async status(takeId: string): Promise<UploadStatus> {
    this.calls.push(`status ${takeId}`);
    const take = this.acked.get(takeId) ?? new Map<number, string>();
    return {
      finalized: this.finalized?.takeId === takeId,
      chunks: [...take].map(([idx, sha256]) => ({ idx, size_bytes: 1, sha256 })),
    };
  }
  finalize = async (takeId: string, chunkCount: number, durationMs: number) => {
    this.calls.push(`finalize ${takeId} ${chunkCount}`);
    this.finalized = { takeId, chunkCount, durationMs };
    return { recording_id: 'rec-final' };
  };
}

const LOCAL = 'local-take';
const put = async () => undefined;

async function orphan(store: MemoryStore, indexes: number[], meta: Partial<TakeMeta> = {}) {
  for (const idx of indexes) {
    await store.put(LOCAL, idx, new Blob([`chunk ${idx}`]));
  }
  await store.putMeta({
    takeId: LOCAL,
    startedAt: 1,
    mimeType: 'video/webm;codecs=vp9,opus',
    chunkCount: indexes.length,
    durationMs: 7_000,
    ...meta,
  });
}

describe('uploadRecoveredTake', () => {
  it('uploads only what the server lacks, finalizes, and clears the device', async () => {
    const store = new MemoryStore();
    const server = new FakeServer();
    await orphan(store, [0, 1, 2, 3], { serverTakeId: 'server-take-9' });
    server.acked.set(
      'server-take-9',
      new Map([
        [0, await sha256Hex(new Blob(['chunk 0']))],
        [1, await sha256Hex(new Blob(['chunk 1']))],
      ]),
    );

    const result = await uploadRecoveredTake({
      api: server,
      store,
      takeId: LOCAL,
      uploader: { put },
    });

    expect(server.calls.filter((call) => call.startsWith('ack'))).toEqual([
      'ack server-take-9 2',
      'ack server-take-9 3',
    ]);
    expect(server.finalized).toEqual({ takeId: 'server-take-9', chunkCount: 4, durationMs: 7_000 });
    expect(result).toEqual({ recording_id: 'rec-final', take_id: 'server-take-9', chunkCount: 4 });
    expect(await store.indexes(LOCAL)).toEqual([]);
    expect(server.created).toBe(0);
  });

  it('creates a recording for a take recorded offline, journaling it first', async () => {
    const store = new MemoryStore();
    const server = new FakeServer();
    await orphan(store, [0, 1]);
    const puts: string[] = [];
    const result = await uploadRecoveredTake({
      api: server,
      store,
      takeId: LOCAL,
      uploader: {
        put: async (url) => {
          // By the time anything is uploaded, the journal names the new server take.
          puts.push(`${url} journaled=${(await store.getMeta(LOCAL))?.serverTakeId}`);
        },
      },
    });
    expect(server.created).toBe(1);
    expect(result.take_id).toBe('server-take-1');
    expect(puts).toEqual([
      'https://s/server-take-1/0 journaled=server-take-1',
      'https://s/server-take-1/1 journaled=server-take-1',
    ]);
    expect(server.finalized?.chunkCount).toBe(2);
  });

  it('resumes the same server take when an offline take is recovered twice', async () => {
    const store = new MemoryStore();
    const server = new FakeServer();
    await orphan(store, [0, 1]);
    // The first attempt creates the recording and uploads, then finalize is refused.
    const finalize = server.finalize.bind(server);
    server.finalize = async () => {
      throw new UploadHttpError('api', 409, 'try again later');
    };
    await expect(
      uploadRecoveredTake({ api: server, store, takeId: LOCAL, uploader: { put } }),
    ).rejects.toBeTruthy();
    expect(await store.indexes(LOCAL)).toEqual([0, 1]);

    server.finalize = finalize;
    const result = await uploadRecoveredTake({
      api: server,
      store,
      takeId: LOCAL,
      uploader: { put },
    });
    expect(server.created).toBe(1);
    expect(result.take_id).toBe('server-take-1');
    expect(server.finalized?.takeId).toBe('server-take-1');
  });

  it('uploads the contiguous run only, like a recovered file plays', async () => {
    const store = new MemoryStore();
    const server = new FakeServer();
    await orphan(store, [0, 1, 3], { serverTakeId: 'server-take-9' });
    const result = await uploadRecoveredTake({
      api: server,
      store,
      takeId: LOCAL,
      uploader: { put },
    });
    expect(result.chunkCount).toBe(2);
    expect(server.calls).not.toContain('ack server-take-9 3');
    expect(server.finalized?.chunkCount).toBe(2);
  });

  it('refuses a take with no usable chunks', async () => {
    const store = new MemoryStore();
    await orphan(store, [1, 2], { serverTakeId: 'server-take-9' });
    await expect(
      uploadRecoveredTake({ api: new FakeServer(), store, takeId: LOCAL, uploader: { put } }),
    ).rejects.toThrow('no usable parts');
  });
});
