import { BehaviorSubject, Observable } from 'rxjs';

/** The slice of `navigator.wakeLock` the guard uses; injectable for tests. */
export interface WakeLockPort {
  request(type: 'screen'): Promise<WakeLockSentinelPort>;
}

export interface WakeLockSentinelPort {
  release(): Promise<void>;
  addEventListener(type: 'release', listener: () => void): void;
}

export type WakeLockState = 'off' | 'held' | 'unsupported';

/** The slice of `document` the wake-lock guard listens to. */
export interface VisibilityPort {
  readonly visibilityState: DocumentVisibilityState;
  addEventListener(type: 'visibilitychange', listener: () => void): void;
  removeEventListener(type: 'visibilitychange', listener: () => void): void;
}

/**
 * Keeps the screen awake while a take runs (a sleeping laptop stops the recording). The browser
 * drops a wake lock whenever the tab is hidden, which is the normal state while another window
 * is being recorded, so the lock is asked for again when the tab becomes visible. Failing to get
 * a lock is never fatal: recording goes on, and `state` says so. Framework-free.
 */
export class WakeLockGuard {
  private sentinel: WakeLockSentinelPort | null = null;
  private wanted = false;
  private readonly stateSubject: BehaviorSubject<WakeLockState>;
  private readonly onVisible = () => {
    if (this.wanted && this.visibility?.visibilityState === 'visible' && !this.sentinel) {
      void this.acquireNow();
    }
  };

  readonly state$: Observable<WakeLockState>;

  constructor(
    private readonly wakeLock: WakeLockPort | undefined = (
      globalThis.navigator as Navigator | undefined
    )?.wakeLock,
    private readonly visibility: VisibilityPort | undefined = globalThis.document,
  ) {
    this.stateSubject = new BehaviorSubject<WakeLockState>(wakeLock ? 'off' : 'unsupported');
    this.state$ = this.stateSubject.asObservable();
  }

  get state(): WakeLockState {
    return this.stateSubject.value;
  }

  async acquire(): Promise<void> {
    if (!this.wakeLock || this.wanted) {
      return;
    }
    this.wanted = true;
    this.visibility?.addEventListener('visibilitychange', this.onVisible);
    await this.acquireNow();
  }

  async release(): Promise<void> {
    this.wanted = false;
    this.visibility?.removeEventListener('visibilitychange', this.onVisible);
    const sentinel = this.sentinel;
    this.sentinel = null;
    if (this.wakeLock) {
      this.stateSubject.next('off');
    }
    await sentinel?.release().catch(() => undefined);
  }

  private async acquireNow(): Promise<void> {
    try {
      const sentinel = await this.wakeLock?.request('screen');
      if (!sentinel) {
        return;
      }
      if (!this.wanted) {
        // Released while the request was pending.
        await sentinel.release().catch(() => undefined);
        return;
      }
      this.sentinel = sentinel;
      this.stateSubject.next('held');
      sentinel.addEventListener('release', () => {
        if (this.sentinel === sentinel) {
          this.sentinel = null;
          if (this.wanted) {
            this.stateSubject.next('off');
          }
        }
      });
    } catch {
      // Denied (battery saver, hidden tab): the take goes on; the lock is retried on visibility.
      this.stateSubject.next(this.wanted ? 'off' : this.stateSubject.value);
    }
  }
}

export interface StorageEstimatePort {
  estimate(): Promise<{ usage?: number; quota?: number }>;
}

/** Warn when this much of the origin's quota is used, or would be by the take (§11). */
export const STORAGE_WARN_FRACTION = 0.8;

export type StorageStatus =
  | { level: 'unknown' }
  | { level: 'ok'; usedFraction: number }
  /** Over `STORAGE_WARN_FRACTION` of the quota already, or once this take is added. */
  | { level: 'low'; usedFraction: number; projectedFraction: number; freeBytes: number };

/**
 * Pure part of the pre-check: `usage`/`quota` bytes from the browser, plus how many bytes the
 * take is expected to add (`expectedBytes`, from the bitrate and the plan's longest take).
 */
export function storageStatus(
  estimate: { usage?: number; quota?: number },
  expectedBytes = 0,
): StorageStatus {
  const { usage, quota } = estimate;
  if (!quota || usage === undefined) {
    return { level: 'unknown' };
  }
  const usedFraction = usage / quota;
  const projectedFraction = (usage + expectedBytes) / quota;
  return projectedFraction >= STORAGE_WARN_FRACTION || usedFraction >= STORAGE_WARN_FRACTION
    ? { level: 'low', usedFraction, projectedFraction, freeBytes: Math.max(0, quota - usage) }
    : { level: 'ok', usedFraction };
}

/** Reads the browser's estimate; `unknown` where the Storage API is missing or fails. */
export async function checkStorage(
  port: StorageEstimatePort | undefined = (globalThis.navigator as Navigator | undefined)?.storage,
  expectedBytes = 0,
): Promise<StorageStatus> {
  try {
    return port ? storageStatus(await port.estimate(), expectedBytes) : { level: 'unknown' };
  } catch {
    return { level: 'unknown' };
  }
}

/** Bytes a take of `durationMs` adds at `bitsPerSecond` (video plus audio). */
export function expectedTakeBytes(
  bitsPerSecond: number,
  durationMs: number,
  audioBitsPerSecond = 128_000,
): number {
  return Math.ceil(((bitsPerSecond + audioBitsPerSecond) * durationMs) / 1000 / 8);
}
