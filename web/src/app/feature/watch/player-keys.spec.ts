import { PLAYBACK_SPEEDS, SEEK_STEP_S, keyAction, seekTarget } from './player-keys';

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
