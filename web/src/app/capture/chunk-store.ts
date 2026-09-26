import { CaptureError } from './capture-error';

/**
 * Durable local storage for a take's chunks, written before anything is uploaded, so that a
 * crashed or closed tab never loses a recording (§10 Record step 7, US-12). OPFS first, with
 * an IndexedDB fallback where OPFS or its writable file handles are missing (§2, ADR 0004 in
 * the design's decision log). Framework-free (CLAUDE.md rule 9).
 */
/**
 * What's known about a take, journaled after every chunk so it survives a crash (Day 26):
 * `durationMs` is the pause-excluding recorded time when the last chunk was persisted.
 */
export interface TakeMeta {
  takeId: string;
  /** Wall-clock start, epoch ms. */
  startedAt: number;
  mimeType: string;
  chunkCount: number;
  durationMs: number;
}

export interface ChunkStore {
  readonly backend: 'opfs' | 'indexeddb';
  /** Stores a chunk; writing the same index again replaces it (idempotent retries). */
  put(takeId: string, index: number, blob: Blob): Promise<void>;
  get(takeId: string, index: number): Promise<Blob | null>;
  /** Stored chunk indexes for a take, ascending. */
  indexes(takeId: string): Promise<number[]>;
  /** Every take with at least one stored chunk. */
  takes(): Promise<string[]>;
  /** Removes a take's chunks and metadata. */
  deleteTake(takeId: string): Promise<void>;
  putMeta(meta: TakeMeta): Promise<void>;
  getMeta(takeId: string): Promise<TakeMeta | null>;
}

const OPFS_ROOT = 'sintade-chunks';
const CHUNK_SUFFIX = '.chunk';
const META_FILE = 'take.json';
const IDB_NAME = 'sintade-chunks';
const IDB_STORE = 'chunks';
const IDB_META_STORE = 'takes';
const IDB_VERSION = 2;
const IDB_TAKE_INDEX = 'by-take';
const TAKE_ID = /^[A-Za-z0-9-]{1,64}$/;

/** Opens the best available store: OPFS if it can write files, else IndexedDB. */
export async function openChunkStore(
  storage: Pick<StorageManager, 'getDirectory'> | undefined = globalThis.navigator?.storage,
  indexedDb: IDBFactory | undefined = globalThis.indexedDB,
): Promise<ChunkStore> {
  const opfs = await openOpfsChunkStore(storage);
  if (opfs) {
    return opfs;
  }
  if (indexedDb) {
    return openIndexedDbChunkStore(indexedDb);
  }
  throw new CaptureError('not-supported', 'neither OPFS nor IndexedDB is available');
}

/**
 * The OPFS store, or `null` where OPFS can't be written from this thread. `name` separates
 * independent stores (e.g. diagnostics) from the app's recordings.
 */
export async function openOpfsChunkStore(
  storage: Pick<StorageManager, 'getDirectory'> | undefined = globalThis.navigator?.storage,
  name = OPFS_ROOT,
): Promise<ChunkStore | null> {
  if (typeof storage?.getDirectory !== 'function') {
    return null;
  }
  try {
    const root = await storage.getDirectory();
    const directory = await root.getDirectoryHandle(name, { create: true });
    // Some engines expose OPFS but only allow writes from workers (sync access handles).
    const probe = await directory.getFileHandle('.probe', { create: true });
    if (typeof probe.createWritable !== 'function') {
      return null;
    }
    await directory.removeEntry('.probe');
    return new OpfsChunkStore(directory);
  } catch {
    // e.g. SecurityError in Firefox private browsing.
    return null;
  }
}

export class OpfsChunkStore implements ChunkStore {
  readonly backend = 'opfs' as const;

  constructor(private readonly root: FileSystemDirectoryHandle) {}

  async put(takeId: string, index: number, blob: Blob): Promise<void> {
    const directory = await this.takeDirectory(takeId, true);
    const file = await directory.getFileHandle(chunkName(index), { create: true });
    const writable = await file.createWritable();
    try {
      await writable.write(blob);
      // close() commits: until then readers still see the previous contents, if any.
      await writable.close();
    } catch (error) {
      await writable.abort().catch(() => undefined);
      throw error;
    }
  }

  async get(takeId: string, index: number): Promise<Blob | null> {
    const directory = await this.takeDirectory(takeId, false);
    if (!directory) {
      return null;
    }
    try {
      const file = await directory.getFileHandle(chunkName(index));
      return await file.getFile();
    } catch (error) {
      if (isNotFound(error)) {
        return null;
      }
      throw error;
    }
  }

  async indexes(takeId: string): Promise<number[]> {
    const directory = await this.takeDirectory(takeId, false);
    if (!directory) {
      return [];
    }
    const indexes: number[] = [];
    for await (const name of directory.keys()) {
      const index = parseChunkName(name);
      if (index !== null) {
        indexes.push(index);
      }
    }
    return indexes.sort((a, b) => a - b);
  }

  async takes(): Promise<string[]> {
    const takes: string[] = [];
    for await (const [name, handle] of this.root.entries()) {
      if (handle.kind === 'directory' && TAKE_ID.test(name)) {
        if ((await this.indexes(name)).length > 0) {
          takes.push(name);
        }
      }
    }
    return takes.sort();
  }

  async putMeta(meta: TakeMeta): Promise<void> {
    const directory = await this.takeDirectory(meta.takeId, true);
    const file = await directory.getFileHandle(META_FILE, { create: true });
    const writable = await file.createWritable();
    try {
      await writable.write(JSON.stringify(meta));
      await writable.close();
    } catch (error) {
      await writable.abort().catch(() => undefined);
      throw error;
    }
  }

  async getMeta(takeId: string): Promise<TakeMeta | null> {
    const directory = await this.takeDirectory(takeId, false);
    if (!directory) {
      return null;
    }
    try {
      const file = await directory.getFileHandle(META_FILE);
      return parseMeta(await (await file.getFile()).text());
    } catch (error) {
      if (isNotFound(error)) {
        return null;
      }
      throw error;
    }
  }

  async deleteTake(takeId: string): Promise<void> {
    validateTakeId(takeId);
    try {
      await this.root.removeEntry(takeId, { recursive: true });
    } catch (error) {
      if (!isNotFound(error)) {
        throw error;
      }
    }
  }

  private async takeDirectory(takeId: string, create: true): Promise<FileSystemDirectoryHandle>;
  private async takeDirectory(
    takeId: string,
    create: false,
  ): Promise<FileSystemDirectoryHandle | null>;
  private async takeDirectory(
    takeId: string,
    create: boolean,
  ): Promise<FileSystemDirectoryHandle | null> {
    validateTakeId(takeId);
    try {
      return await this.root.getDirectoryHandle(takeId, { create });
    } catch (error) {
      if (!create && isNotFound(error)) {
        return null;
      }
      throw error;
    }
  }
}

export async function openIndexedDbChunkStore(
  indexedDb: IDBFactory,
  name = IDB_NAME,
): Promise<ChunkStore> {
  const db = await request(
    (() => {
      const open = indexedDb.open(name, IDB_VERSION);
      open.onupgradeneeded = (event) => {
        // Forward-only upgrades, like the server's migrations.
        if (event.oldVersion < 1) {
          const store = open.result.createObjectStore(IDB_STORE, {
            keyPath: ['takeId', 'index'],
          });
          store.createIndex(IDB_TAKE_INDEX, 'takeId');
        }
        if (event.oldVersion < 2) {
          open.result.createObjectStore(IDB_META_STORE, { keyPath: 'takeId' });
        }
      };
      return open;
    })(),
  );
  return new IndexedDbChunkStore(db);
}

/**
 * Bytes, not the Blob itself: WebKit refuses Blobs in IndexedDB in ephemeral/private contexts
 * (the transaction just aborts), while an ArrayBuffer works everywhere.
 */
interface StoredChunk {
  takeId: string;
  index: number;
  bytes: ArrayBuffer;
  type: string;
}

class IndexedDbChunkStore implements ChunkStore {
  readonly backend = 'indexeddb' as const;

  constructor(private readonly db: IDBDatabase) {}

  async put(takeId: string, index: number, blob: Blob): Promise<void> {
    validateTakeId(takeId);
    validateIndex(index);
    const record: StoredChunk = { takeId, index, bytes: await blob.arrayBuffer(), type: blob.type };
    await this.transact('readwrite', (store) => store.put(record));
  }

  async get(takeId: string, index: number): Promise<Blob | null> {
    validateTakeId(takeId);
    const record = await this.transact<StoredChunk | undefined>('readonly', (store) =>
      store.get([takeId, index]),
    );
    return record ? new Blob([record.bytes], { type: record.type }) : null;
  }

  async indexes(takeId: string): Promise<number[]> {
    validateTakeId(takeId);
    const keys = await this.transact<IDBValidKey[]>('readonly', (store) =>
      store.index(IDB_TAKE_INDEX).getAllKeys(IDBKeyRange.only(takeId)),
    );
    return keys.map((key) => (key as [string, number])[1]).sort((a, b) => a - b);
  }

  async takes(): Promise<string[]> {
    const keys = await this.transact<IDBValidKey[]>('readonly', (store) => store.getAllKeys());
    return [...new Set(keys.map((key) => (key as [string, number])[0]))].sort();
  }

  async deleteTake(takeId: string): Promise<void> {
    validateTakeId(takeId);
    await this.transact('readwrite', (store) =>
      store.delete(IDBKeyRange.bound([takeId, 0], [takeId, Number.MAX_SAFE_INTEGER])),
    );
    await this.transact('readwrite', (store) => store.delete(takeId), IDB_META_STORE);
  }

  async putMeta(meta: TakeMeta): Promise<void> {
    validateTakeId(meta.takeId);
    await this.transact('readwrite', (store) => store.put({ ...meta }), IDB_META_STORE);
  }

  async getMeta(takeId: string): Promise<TakeMeta | null> {
    validateTakeId(takeId);
    const record = await this.transact<TakeMeta | undefined>(
      'readonly',
      (store) => store.get(takeId),
      IDB_META_STORE,
    );
    return record ?? null;
  }

  /** Runs one request in its own transaction; resolves once the transaction has committed. */
  private transact<T>(
    mode: IDBTransactionMode,
    run: (store: IDBObjectStore) => IDBRequest,
    storeName = IDB_STORE,
  ): Promise<T> {
    return new Promise<T>((resolve, reject) => {
      const transaction = this.db.transaction(storeName, mode);
      const pending = run(transaction.objectStore(storeName));
      const failure = () =>
        transaction.error ??
        pending.error ??
        new CaptureError('unknown', `IndexedDB ${mode} transaction on chunks failed`);
      transaction.oncomplete = () => resolve(pending.result as T);
      transaction.onerror = () => reject(failure());
      transaction.onabort = () => reject(failure());
    });
  }
}

function request<T>(pending: IDBRequest<T>): Promise<T> {
  return new Promise((resolve, reject) => {
    pending.onsuccess = () => resolve(pending.result);
    pending.onerror = () => reject(pending.error);
  });
}

/** Zero-padded so a plain directory listing sorts in index order. */
export function chunkName(index: number): string {
  validateIndex(index);
  return `${String(index).padStart(6, '0')}${CHUNK_SUFFIX}`;
}

export function parseChunkName(name: string): number | null {
  const match = /^(\d{6,})\.chunk$/.exec(name);
  return match ? Number(match[1]) : null;
}

function parseMeta(json: string): TakeMeta | null {
  try {
    const value: unknown = JSON.parse(json);
    if (
      typeof value === 'object' &&
      value !== null &&
      'takeId' in value &&
      'startedAt' in value &&
      'durationMs' in value
    ) {
      return value as TakeMeta;
    }
  } catch {
    // A torn write can't happen (writes commit on close), but never let bad JSON block recovery.
  }
  return null;
}

function validateTakeId(takeId: string): void {
  if (!TAKE_ID.test(takeId)) {
    throw new CaptureError('unknown', `invalid take id: ${JSON.stringify(takeId)}`);
  }
}

function validateIndex(index: number): void {
  if (!Number.isSafeInteger(index) || index < 0) {
    throw new CaptureError('unknown', `invalid chunk index: ${index}`);
  }
}

function isNotFound(error: unknown): boolean {
  return error instanceof Error && error.name === 'NotFoundError';
}
