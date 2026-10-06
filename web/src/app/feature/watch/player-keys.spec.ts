import {
  LONG_SEEK_STEP_S,
  PLAYBACK_SPEEDS,
  SEEK_STEP_S,
  keyAction,
  seekTarget,
  stepSpeed,
  stepVolume,
} from './player-keys';

describe('keyAction', () => {
  it('seeks five seconds with the arrow keys', () => {
    expect(keyAction({ key: 'ArrowLeft' })).toEqual({ kind: 'seek', deltaS: -SEEK_STEP_S });
    expect(keyAction({ key: 'ArrowRight' })).toEqual({ kind: 'seek', deltaS: SEEK_STEP_S });
    expect(SEEK_STEP_S).toBe(5);
  });

  it('maps play, fullscreen and mute keys', () => {
    expect(keyAction({ key: ' ' })).toEqual({ kind: 'toggle-play' });
    expect(keyAction({ key: 'k' })).toEqual({ kind: 'toggle-play' });
    expect(keyAction({ key: 'f' })).toEqual({ kind: 'fullscreen' });
    expect(keyAction({ key: 'm' })).toEqual({ kind: 'mute' });
  });

  it('leaves browser shortcuts and other keys alone', () => {
    expect(keyAction({ key: 'ArrowRight', ctrlKey: true })).toBeNull();
    expect(keyAction({ key: 'f', metaKey: true })).toBeNull();
    expect(keyAction({ key: 'a' })).toBeNull();
  });
});

describe('seekTarget', () => {
  it('moves by the delta', () => {
    expect(seekTarget(20, 5, 60)).toBe(25);
    expect(seekTarget(20, -5, 60)).toBe(15);
  });

  it('stays inside the video', () => {
    expect(seekTarget(2, -5, 60)).toBe(0);
    expect(seekTarget(58, 5, 60)).toBe(60);
  });

  it('does not cap while the duration is unknown', () => {
    expect(seekTarget(10, 5, Number.NaN)).toBe(15);
    expect(seekTarget(10, 5, Number.POSITIVE_INFINITY)).toBe(15);
  });
});

describe('PLAYBACK_SPEEDS', () => {
  it('spans 0.5x to 2x and includes normal speed', () => {
    expect(PLAYBACK_SPEEDS[0]).toBe(0.5);
    expect(PLAYBACK_SPEEDS[PLAYBACK_SPEEDS.length - 1]).toBe(2);
    expect(PLAYBACK_SPEEDS).toContain(1);
  });
});

describe('more shortcuts', () => {
  it('seeks ten seconds with J and L, to the ends with Home and End', () => {
    expect(keyAction({ key: 'j' })).toEqual({ kind: 'seek', deltaS: -LONG_SEEK_STEP_S });
    expect(keyAction({ key: 'l' })).toEqual({ kind: 'seek', deltaS: LONG_SEEK_STEP_S });
    expect(keyAction({ key: 'Home' })).toEqual({ kind: 'seek-edge', edge: 'start' });
    expect(keyAction({ key: 'End' })).toEqual({ kind: 'seek-edge', edge: 'end' });
  });

  it('jumps to tenths of the video with the digits', () => {
    expect(keyAction({ key: '0' })).toEqual({ kind: 'seek-to', fraction: 0 });
    expect(keyAction({ key: '5' })).toEqual({ kind: 'seek-to', fraction: 0.5 });
    expect(keyAction({ key: '9' })).toEqual({ kind: 'seek-to', fraction: 0.9 });
  });

  it('steps speed with < and > and volume with the arrows', () => {
    expect(keyAction({ key: '>' })).toEqual({ kind: 'speed', direction: 1 });
    expect(keyAction({ key: '<' })).toEqual({ kind: 'speed', direction: -1 });
    expect(keyAction({ key: 'ArrowUp' })?.kind).toBe('volume');
    expect(keyAction({ key: 'ArrowDown' })).toEqual({ kind: 'volume', delta: -0.05 });
  });

  it('keeps speed and volume inside their ranges', () => {
    expect(stepSpeed(1, 1)).toBe(1.25);
    expect(stepSpeed(1, -1)).toBe(0.75);
    expect(stepSpeed(2, 1)).toBe(2);
    expect(stepSpeed(0.5, -1)).toBe(0.5);
    expect(stepVolume(0.98, 0.05)).toBe(1);
    expect(stepVolume(0.02, -0.05)).toBe(0);
  });
});
