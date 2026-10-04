/** How far ← and → seek, in seconds. */
export const SEEK_STEP_S = 5;

/** The speeds the player offers (docs/design.md §4.6: 0.5–2×). */
export const PLAYBACK_SPEEDS = [0.5, 0.75, 1, 1.25, 1.5, 2] as const;

export type KeyAction =
  | { kind: 'seek'; deltaS: number }
  | { kind: 'toggle-play' }
  | { kind: 'fullscreen' }
  | { kind: 'mute' };

/** What a key press does in the player, or `null` if the player ignores it. */
export function keyAction(event: {
  key: string;
  ctrlKey?: boolean;
  metaKey?: boolean;
  altKey?: boolean;
}): KeyAction | null {
  if (event.ctrlKey || event.metaKey || event.altKey) {
    return null;
  }
  switch (event.key) {
    case 'ArrowLeft':
      return { kind: 'seek', deltaS: -SEEK_STEP_S };
    case 'ArrowRight':
      return { kind: 'seek', deltaS: SEEK_STEP_S };
    case ' ':
    case 'k':
      return { kind: 'toggle-play' };
    case 'f':
      return { kind: 'fullscreen' };
    case 'm':
      return { kind: 'mute' };
    default:
      return null;
  }
}

/** Where a seek of `deltaS` from `current` lands, kept inside the video. */
export function seekTarget(current: number, deltaS: number, duration: number): number {
  const end = Number.isFinite(duration) && duration > 0 ? duration : Number.POSITIVE_INFINITY;
  return Math.min(Math.max(0, current + deltaS), end);
}
