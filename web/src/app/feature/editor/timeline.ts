/**
 * The editor's timeline (ADR-0040): one horizontal axis that spans the original recording,
 * start to end, so its left edge is 0 and its right edge is exactly the recording's length.
 * Framework-free so the mapping can be tested on its own.
 */
import { SpriteCue, cueAt } from '../watch/sprite-preview';

/** Where `ms` sits along a timeline of `durationMs`, from 0 (start) to 1 (end). */
export function msToFraction(ms: number, durationMs: number): number {
  if (durationMs <= 0) {
    return 0;
  }
  return Math.min(1, Math.max(0, ms / durationMs));
}

/** The time at `fraction` (0–1) of a timeline of `durationMs`. */
export function fractionToMs(fraction: number, durationMs: number): number {
  return Math.round(Math.min(1, Math.max(0, fraction)) * Math.max(0, durationMs));
}

/** One thumbnail of the strip: the cue shown and the stretch of the recording it stands for. */
export interface StripThumbnail {
  cue: SpriteCue;
  startMs: number;
  endMs: number;
}

/**
 * `count` thumbnails side by side across the whole recording: thumbnail `i` covers
 * `[i, i + 1) / count` of it and shows the sprite cue at its middle.
 */
export function thumbnailStrip(
  cues: readonly SpriteCue[],
  durationMs: number,
  count: number,
): StripThumbnail[] {
  const slots = Math.max(0, Math.floor(count));
  if (cues.length === 0 || durationMs <= 0) {
    return [];
  }
  const strip: StripThumbnail[] = [];
  for (let i = 0; i < slots; i++) {
    const startMs = (i * durationMs) / slots;
    const endMs = ((i + 1) * durationMs) / slots;
    const cue = cueAt(cues, (startMs + endMs) / 2000);
    if (cue) {
      strip.push({ cue, startMs, endMs });
    }
  }
  return strip;
}

/** How many thumbnails of `cue`'s shape, `height` px tall, fit side by side in `width` px. */
export function thumbnailCount(width: number, height: number, cue: SpriteCue | undefined): number {
  if (!cue || cue.height <= 0 || height <= 0 || width <= 0) {
    return 0;
  }
  const thumbWidth = (cue.width * height) / cue.height;
  return Math.max(1, Math.round(width / thumbWidth));
}
