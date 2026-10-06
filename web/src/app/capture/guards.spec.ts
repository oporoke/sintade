import {
  WakeLockGuard,
  WakeLockPort,
  checkStorage,
  expectedTakeBytes,
  storageStatus,
} from './guards';

class FakeSentinel {
  private listener: (() => void) | null = null;
  release = vi.fn(async () => this.listener?.());
  addEventListener(_type: 'release', listener: () => void) {
    this.listener = listener;
  }
  /** What the browser does when the tab is hidden. */
  dropFromBrowser() {
    this.listener?.();
  }
}

class FakeDocument {
  visibilityState: DocumentVisibilityState = 'visible';
  private listeners = new Set<() => void>();
  addEventListener(_t: 'visibilitychange', l: () => void) {
    this.listeners.add(l);
  }
  removeEventListener(_t: 'visibilitychange', l: () => void) {
    this.listeners.delete(l);
  }
  show() {
    this.visibilityState = 'visible';
    this.listeners.forEach((l) => l());
  }
  get listenerCount() {
    return this.listeners.size;
  }
}

function setup(request?: () => Promise<FakeSentinel>) {
  const sentinels: FakeSentinel[] = [];
  const port = {
    request: vi.fn(
      request ??
        (async () => {
          const sentinel = new FakeSentinel();
          sentinels.push(sentinel);
          return sentinel;
        }),
    ),
  };
  const doc = new FakeDocument();
  const guard = new WakeLockGuard(port as unknown as WakeLockPort, doc);
  return { guard, port, sentinels, doc };
}

describe('WakeLockGuard', () => {
  it('holds a screen wake lock while wanted and lets go on release', async () => {
    const { guard, port, sentinels, doc } = setup();
    await guard.acquire();
    expect(port.request).toHaveBeenCalledWith('screen');
    expect(guard.state).toBe('held');
    await guard.release();
    expect(sentinels[0].release).toHaveBeenCalled();
    expect(guard.state).toBe('off');
    expect(doc.listenerCount).toBe(0);
  });

  it('asks again when the tab becomes visible after the browser dropped the lock', async () => {
    const { guard, port, sentinels, doc } = setup();
    await guard.acquire();
    doc.visibilityState = 'hidden';
    sentinels[0].dropFromBrowser();
    expect(guard.state).toBe('off');
    doc.show();
    await vi.waitFor(() => expect(guard.state).toBe('held'));
    expect(port.request).toHaveBeenCalledTimes(2);
  });

  it('reports unsupported without a Wake Lock API, and a refusal is not fatal', async () => {
    const none = new WakeLockGuard(undefined, new FakeDocument());
    await none.acquire();
    expect(none.state).toBe('unsupported');

    const { guard } = setup(async () => {
      throw new DOMException('denied', 'NotAllowedError');
    });
    await expect(guard.acquire()).resolves.toBeUndefined();
    expect(guard.state).toBe('off');
  });

  it('releases a lock that arrives after release() was called', async () => {
    let resolve: (s: FakeSentinel) => void = () => undefined;
    const late = new FakeSentinel();
    const { guard } = setup(() => new Promise((r) => (resolve = r)));
    const pending = guard.acquire();
    await guard.release();
    resolve(late);
    await pending;
    expect(late.release).toHaveBeenCalled();
    expect(guard.state).toBe('off');
  });
});

describe('storageStatus', () => {
  it('warns from 80 % of the quota used', () => {
    expect(storageStatus({ usage: 700, quota: 1000 })).toEqual({ level: 'ok', usedFraction: 0.7 });
    expect(storageStatus({ usage: 800, quota: 1000 })).toMatchObject({
      level: 'low',
      usedFraction: 0.8,
      freeBytes: 200,
    });
  });

  it('warns when the take itself would take it past 80 %', () => {
    expect(storageStatus({ usage: 500, quota: 1000 }, 350)).toMatchObject({
      level: 'low',
      projectedFraction: 0.85,
    });
    expect(storageStatus({ usage: 500, quota: 1000 }, 200).level).toBe('ok');
  });

  it('is unknown without numbers, and when the API fails or is missing', async () => {
    expect(storageStatus({})).toEqual({ level: 'unknown' });
    expect(await checkStorage(undefined)).toEqual({ level: 'unknown' });
    expect(
      await checkStorage({
        estimate: async () => {
          throw new Error('nope');
        },
      }),
    ).toEqual({ level: 'unknown' });
    expect(
      await checkStorage({ estimate: async () => ({ usage: 900, quota: 1000 }) }),
    ).toMatchObject({ level: 'low' });
  });

  it('estimates a take size from bitrate and length', () => {
    // 5 Mbit/s video + 128 kbit/s audio for 10 minutes.
    expect(expectedTakeBytes(5_000_000, 600_000)).toBe(384_600_000);
  });
});
