/** How far ← and → seek, in seconds. */
export const SEEK_STEP_S = 5;

/** How far J and L seek, in seconds. */
export const LONG_SEEK_STEP_S = 10;

/** Volume change per ↑/↓ press. */
export const VOLUME_STEP = 0.05;

/** The speeds the player offers (docs/design.md §4.6: 0.5–2×). */
export const PLAYBACK_SPEEDS = [0.5, 0.75, 1, 1.25, 1.5, 2] as const;

export type KeyAction =
  | { kind: 'seek'; deltaS: number }
  | { kind: 'toggle-play' }
  | { kind: 'fullscreen' }
  | { kind: 'mute' }
  | { kind: 'seek-to'; fraction: number }
  | { kind: 'seek-edge'; edge: 'start' | 'end' }
  | { kind: 'speed'; direction: -1 | 1 }
  | { kind: 'volume'; delta: number };

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
    case 'j':
      return { kind: 'seek', deltaS: -LONG_SEEK_STEP_S };
    case 'l':
      return { kind: 'seek', deltaS: LONG_SEEK_STEP_S };
    case 'Home':
      return { kind: 'seek-edge', edge: 'start' };
    case 'End':
      return { kind: 'seek-edge', edge: 'end' };
    case '<':
      return { kind: 'speed', direction: -1 };
    case '>':
      return { kind: 'speed', direction: 1 };
    case 'ArrowUp':
      return { kind: 'volume', delta: VOLUME_STEP };
    case 'ArrowDown':
      return { kind: 'volume', delta: -VOLUME_STEP };
    case 'f':
      return { kind: 'fullscreen' };
    case 'm':
      return { kind: 'mute' };
    default:
      if (/^[0-9]$/.test(event.key)) {
        return { kind: 'seek-to', fraction: Number(event.key) / 10 };
      }
      return null;
  }
}

/** Where a seek of `deltaS` from `current` lands, kept inside the video. */
export function seekTarget(current: number, deltaS: number, duration: number): number {
  const end = Number.isFinite(duration) && duration > 0 ? duration : Number.POSITIVE_INFINITY;
  return Math.min(Math.max(0, current + deltaS), end);
}

/** The speed one step from `current` in `direction`, staying within [`PLAYBACK_SPEEDS`]. */
export function stepSpeed(current: number, direction: -1 | 1): number {
  const at = PLAYBACK_SPEEDS.findIndex((speed) => speed >= current);
  const index = at < 0 ? PLAYBACK_SPEEDS.length - 1 : at;
  return PLAYBACK_SPEEDS[Math.min(Math.max(index + direction, 0), PLAYBACK_SPEEDS.length - 1)];
}

/** `volume` moved by `delta`, kept in 0–1. */
export function stepVolume(volume: number, delta: number): number {
  return Math.round(Math.min(Math.max(volume + delta, 0), 1) * 100) / 100;
}
