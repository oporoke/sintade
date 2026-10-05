import { describe, expect, it } from 'vitest';

import { clock, errorActions } from './popup';

describe('clock', () => {
  it('counts like a stopwatch', () => {
    expect(clock(0)).toBe('0:00');
    expect(clock(5_900)).toBe('0:05');
    expect(clock(61_000)).toBe('1:01');
    expect(clock(12 * 60_000 + 3_000)).toBe('12:03');
    expect(clock(-5)).toBe('0:00');
  });
});

describe('errorActions', () => {
  it('offers the way out of each failure', () => {
    expect(errorActions('not-signed-in')).toEqual({ signIn: true, library: false, retry: false });
    expect(errorActions('limit-reached')).toEqual({ signIn: false, library: true, retry: false });
    expect(errorActions('upload-failed')).toEqual({ signIn: false, library: true, retry: true });
    for (const code of ['unreachable', 'capture-failed', 'microphone'] as const) {
      expect(errorActions(code)).toEqual({ signIn: false, library: false, retry: true });
    }
  });
});
