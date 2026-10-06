/** A viewer who gets this close to the end starts over next time (less for a short video). */
export const RESUME_END_MARGIN_S = 5;

function endMargin(durationS: number): number {
  return Number.isFinite(durationS) ? Math.min(RESUME_END_MARGIN_S, durationS * 0.1) : 0;
}
/** Positions this early aren't worth resuming. */
export const RESUME_MIN_S = 3;

const KEY = (slug: string) => `sintade.resume.${slug}`;

function storage(): Storage | null {
  try {
    return globalThis.localStorage ?? null;
  } catch {
    return null;
  }
}

/** Where to resume `slug`, or `null`. Kept in this browser only (no account needed to watch). */
export function loadResume(slug: string, durationS: number): number | null {
  try {
    const raw = storage()?.getItem(KEY(slug));
    const at = raw === null || raw === undefined ? NaN : Number(raw);
    if (!Number.isFinite(at) || at < RESUME_MIN_S) {
      return null;
    }
    if (Number.isFinite(durationS) && at > durationS - endMargin(durationS)) {
      return null;
    }
    return at;
  } catch {
    return null;
  }
}

/** Remembers `positionS`; a position at the very end forgets it instead. */
export function saveResume(slug: string, positionS: number, durationS: number): void {
  try {
    const store = storage();
    if (!store) {
      return;
    }
    const finished = Number.isFinite(durationS) && positionS > durationS - endMargin(durationS);
    if (finished || positionS < RESUME_MIN_S) {
      store.removeItem(KEY(slug));
    } else {
      store.setItem(KEY(slug), String(Math.floor(positionS * 10) / 10));
    }
  } catch {
    // Private mode or a full quota: resuming is a convenience.
  }
}
