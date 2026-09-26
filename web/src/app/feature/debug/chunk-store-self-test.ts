import { ChunkStore, openIndexedDbChunkStore, openOpfsChunkStore } from '../../capture';

export type StoreBackend = ChunkStore['backend'];

export interface StoredTakeCheck {
  takeId: string;
  indexes: number[];
  /** Every chunk's bytes match what was written. */
  intact: boolean;
}

const TAKE_PREFIX = 'selftest-';
const CHUNKS = 5;

/**
 * Day 25 Check, "chunks survive a page reload": write a test take through one backend, reload,
 * then read it back and compare every byte. Chunk content is derived from (take, index), so the
 * verifying page needs nothing from the writing page.
 */
/** Its own store, so test takes never look like the user's unfinished recordings. */
const SELF_TEST_STORE = 'sintade-chunks-selftest';

export async function openBackend(backend: StoreBackend): Promise<ChunkStore | null> {
  return backend === 'opfs'
    ? openOpfsChunkStore(navigator.storage, SELF_TEST_STORE)
    : openIndexedDbChunkStore(indexedDB, SELF_TEST_STORE);
}

export async function writeTestTake(store: ChunkStore): Promise<string> {
  const takeId = `${TAKE_PREFIX}${Date.now()}`;
  for (let index = 0; index < CHUNKS; index += 1) {
    await store.put(takeId, index, new Blob([expectedBytes(takeId, index)]));
  }
  return takeId;
}

export async function inspectTestTakes(store: ChunkStore): Promise<StoredTakeCheck[]> {
  const takes = (await store.takes()).filter((takeId) => takeId.startsWith(TAKE_PREFIX));
  return Promise.all(
    takes.map(async (takeId) => {
      const indexes = await store.indexes(takeId);
      const intact = (
        await Promise.all(
          indexes.map(async (index) => {
            const blob = await store.get(takeId, index);
            return blob !== null && sameBytes(await blobBytes(blob), expectedBytes(takeId, index));
          }),
        )
      ).every(Boolean);
      return { takeId, indexes, intact };
    }),
  );
}

export async function clearTestTakes(store: ChunkStore): Promise<void> {
  for (const takeId of await store.takes()) {
    if (takeId.startsWith(TAKE_PREFIX)) {
      await store.deleteTake(takeId);
    }
  }
}

/** 64 KiB plus a per-index amount of pseudo-random but reproducible bytes. */
function expectedBytes(takeId: string, index: number): Uint8Array<ArrayBuffer> {
  let seed = [...`${takeId}#${index}`].reduce((hash, c) => (hash * 31 + c.charCodeAt(0)) >>> 0, 7);
  const bytes = new Uint8Array(65_536 + index * 1_000);
  for (let i = 0; i < bytes.length; i += 1) {
    seed = (seed * 1_664_525 + 1_013_904_223) >>> 0;
    bytes[i] = seed >>> 24;
  }
  return bytes;
}

async function blobBytes(blob: Blob): Promise<Uint8Array> {
  return new Uint8Array(await blob.arrayBuffer());
}

function sameBytes(a: Uint8Array, b: Uint8Array): boolean {
  return a.length === b.length && a.every((value, i) => value === b[i]);
}
