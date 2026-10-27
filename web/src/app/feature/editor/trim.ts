/**
 * The editor's trim (ADR-0041): which stretch of the original is kept, from the start handle to
 * the end handle, in the original's time. Framework-free, like the rest of the timeline maths.
 */

/** The shortest stretch that can be kept: the API's shortest range (`MIN_RANGE_MS`). */
export const MIN_KEPT_MS = 100;

export interface Trim {
  startMs: number;
  endMs: number;
}

/** Nothing trimmed: the whole recording. */
export function fullTrim(durationMs: number): Trim {
  return { startMs: 0, endMs: Math.max(0, durationMs) };
}

/** `trim` with its start handle at `ms`, kept within the recording and before the end handle. */
export function moveStart(trim: Trim, ms: number, durationMs: number): Trim {
  const latest = Math.max(0, Math.min(trim.endMs, durationMs) - MIN_KEPT_MS);
  return { ...trim, startMs: Math.round(Math.min(latest, Math.max(0, ms))) };
}

/** `trim` with its end handle at `ms`, kept within the recording and after the start handle. */
export function moveEnd(trim: Trim, ms: number, durationMs: number): Trim {
  const earliest = Math.min(durationMs, trim.startMs + MIN_KEPT_MS);
  return { ...trim, endMs: Math.round(Math.max(earliest, Math.min(durationMs, ms))) };
}

export function isTrimmed(trim: Trim, durationMs: number): boolean {
  return trim.startMs > 0 || trim.endMs < durationMs;
}

/**
 * How close to the end handle counts as being at it. A video paused at the end handle reports a
 * time a hair before or after it; playing again from there starts over at the start handle.
 */
export const END_SLACK_MS = 50;

/** What the preview does to the video at `currentMs`. */
export type PreviewAction = { kind: 'none' } | { kind: 'seek'; ms: number } | { kind: 'stop' };

/**
 * The preview rule. When playback starts outside the kept stretch (or at its very end) it jumps to
 * the start handle; while playing, reaching the end handle stops it there.
 */
export function previewAction(
  event: 'play' | 'frame',
  currentMs: number,
  trim: Trim,
): PreviewAction {
  if (event === 'play') {
    return currentMs < trim.startMs || currentMs >= trim.endMs - END_SLACK_MS
      ? { kind: 'seek', ms: trim.startMs }
      : { kind: 'none' };
  }
  return currentMs >= trim.endMs ? { kind: 'stop' } : { kind: 'none' };
}
