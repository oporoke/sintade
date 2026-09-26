import { OpfsChunkStore, chunkName, openChunkStore, parseChunkName } from './chunk-store';

function notFound(name: string): Error {
  return Object.assign(new Error(`${name} not found`), { name: 'NotFoundError' });
}

/** In-memory stand-in for OPFS; writes only become visible on close(), like the real thing. */
class FakeFile {
  readonly kind = 'file';
  content: Blob = new Blob([]);
  constructor(private readonly writable = true) {}
  async getFile(): Promise<Blob> {
    return this.content;
  }
  get createWritable() {
    if (!this.writable) {
      return undefined;
    }
    return async () => {
      let pending: Blob = new Blob([]);
      return {
        write: async (data: Blob) => {
          pending = data;
        },
        close: async () => {
          this.content = pending;
        },
        abort: async () => undefined,
      };
    };
  }
}

class FakeDirectory {
  readonly kind = 'directory';
  readonly children = new Map<string, FakeDirectory | FakeFile>();
  constructor(private readonly writableFiles = true) {}
  async getDirectoryHandle(name: string, options?: { create?: boolean }) {
    let child = this.children.get(name);
    if (!child && options?.create) {
      child = new FakeDirectory(this.writableFiles);
      this.children.set(name, child);
    }
    if (!(child instanceof FakeDirectory)) {
      throw notFound(name);
    }
    return child;
  }
  async getFileHandle(name: string, options?: { create?: boolean }) {
    let child = this.children.get(name);
    if (!child && options?.create) {
      child = new FakeFile(this.writableFiles);
      this.children.set(name, child);
    }
    if (!(child instanceof FakeFile)) {
      throw notFound(name);
    }
    return child;
  }
  async removeEntry(name: string) {
    if (!this.children.delete(name)) {
      throw notFound(name);
    }
  }
  async *keys() {
    yield* this.children.keys();
  }
  async *entries() {
    yield* this.children.entries();
  }
}

const TAKE = '0190a1b2-c3d4-7e5f-8a9b-0c1d2e3f4a5b';
const asHandle = (directory: FakeDirectory) => directory as unknown as FileSystemDirectoryHandle;
const text = async (blob: Blob | null) => (blob ? await blob.text() : null);

describe('chunk names', () => {
  it('zero-pads so listings sort in index order, and round-trips', () => {
    expect(chunkName(7)).toBe('000007.chunk');
    expect(parseChunkName('000007.chunk')).toBe(7);
    expect(parseChunkName('1234567.chunk')).toBe(1234567);
    expect(parseChunkName('.probe')).toBeNull();
  });

  it('rejects negative or fractional indexes', () => {
    expect(() => chunkName(-1)).toThrow(/invalid chunk index/);
    expect(() => chunkName(1.5)).toThrow(/invalid chunk index/);
  });
});

describe('OpfsChunkStore', () => {
  function setup() {
    const root = new FakeDirectory();
    return { root, store: new OpfsChunkStore(asHandle(root)) };
  }

  it('stores and reads back chunks per take', async () => {
    const { store } = setup();
    await store.put(TAKE, 0, new Blob(['zero']));
    await store.put(TAKE, 1, new Blob(['one']));

    expect(await text(await store.get(TAKE, 0))).toBe('zero');
    expect(await text(await store.get(TAKE, 1))).toBe('one');
    expect(await store.get(TAKE, 2)).toBeNull();
    expect(await store.get('0190aaaa-0000-7000-8000-000000000000', 0)).toBeNull();
  });

  it('lists indexes in numeric order and ignores foreign files', async () => {
    const { root, store } = setup();
    for (const index of [10, 2, 0]) {
      await store.put(TAKE, index, new Blob([`c${index}`]));
    }
    (await root.getDirectoryHandle(TAKE)).children.set('notes.txt', new FakeFile());
    expect(await store.indexes(TAKE)).toEqual([0, 2, 10]);
  });

  it('overwrites the same index (idempotent retry)', async () => {
    const { store } = setup();
    await store.put(TAKE, 0, new Blob(['first']));
    await store.put(TAKE, 0, new Blob(['second']));
    expect(await text(await store.get(TAKE, 0))).toBe('second');
    expect(await store.indexes(TAKE)).toEqual([0]);
  });

  it('lists takes with chunks and deletes a take entirely', async () => {
    const { store } = setup();
    const other = '0190ffff-0000-7000-8000-000000000001';
    await store.put(TAKE, 0, new Blob(['a']));
    await store.put(other, 0, new Blob(['b']));
    expect(await store.takes()).toEqual([TAKE, other].sort());

    await store.deleteTake(TAKE);
    expect(await store.takes()).toEqual([other]);
    await expect(store.deleteTake(TAKE)).resolves.toBeUndefined(); // already gone: fine
  });

  it('rejects take ids that are not safe file names', async () => {
    const { store } = setup();
    await expect(store.put('../escape', 0, new Blob(['x']))).rejects.toThrow(/invalid take id/);
    await expect(store.indexes('a/b')).rejects.toThrow(/invalid take id/);
  });
});

describe('openChunkStore', () => {
  const storageWith = (root: FakeDirectory) => ({
    getDirectory: async () => asHandle(root),
  });
  const fakeIndexedDb = {} as IDBFactory;

  it('uses OPFS when files are writable', async () => {
    const store = await openChunkStore(storageWith(new FakeDirectory()), fakeIndexedDb);
    expect(store.backend).toBe('opfs');
  });

  it('falls back when OPFS files cannot be written from this thread', async () => {
    const indexedDb = { open: vi.fn(() => ({})) } as unknown as IDBFactory;
    void openChunkStore(storageWith(new FakeDirectory(false)), indexedDb);
    await vi.waitFor(() => expect(indexedDb.open).toHaveBeenCalledWith('sintade-chunks', 1));
  });

  it('falls back when OPFS throws (e.g. private browsing)', async () => {
    const indexedDb = { open: vi.fn(() => ({})) } as unknown as IDBFactory;
    const storage = {
      getDirectory: async () => {
        throw Object.assign(new Error('denied'), { name: 'SecurityError' });
      },
    };
    void openChunkStore(storage, indexedDb);
    await vi.waitFor(() => expect(indexedDb.open).toHaveBeenCalled());
  });

  it('is not-supported with neither backend', async () => {
    await expect(openChunkStore(undefined, undefined)).rejects.toMatchObject({
      kind: 'not-supported',
    });
  });
});
