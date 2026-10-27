import { MIN_KEPT_MS, fullTrim, isTrimmed, moveEnd, moveStart, previewAction } from './trim';

describe('trim', () => {
  it('starts with the whole recording kept', () => {
    expect(fullTrim(12_000)).toEqual({ startMs: 0, endMs: 12_000 });
    expect(isTrimmed(fullTrim(12_000), 12_000)).toBe(false);
  });

  it('keeps each handle inside the recording and on its own side of the other', () => {
    const trim = fullTrim(12_000);
    expect(moveStart(trim, 3_000.4, 12_000)).toEqual({ startMs: 3_000, endMs: 12_000 });
    expect(moveStart(trim, -50, 12_000).startMs).toBe(0);
    expect(moveEnd(trim, 9_000, 12_000)).toEqual({ startMs: 0, endMs: 9_000 });
    expect(moveEnd(trim, 15_000, 12_000).endMs).toBe(12_000);
    // A handle can't pass the other: at least MIN_KEPT_MS stays between them.
    const narrow = { startMs: 3_000, endMs: 9_000 };
    expect(moveStart(narrow, 10_000, 12_000).startMs).toBe(9_000 - MIN_KEPT_MS);
    expect(moveEnd(narrow, 1_000, 12_000).endMs).toBe(3_000 + MIN_KEPT_MS);
    expect(isTrimmed(narrow, 12_000)).toBe(true);
  });

  // Day 94 Check, the rule: the preview starts at the start handle and ends at the end handle.
  it('starts playback at the start handle and stops it at the end handle', () => {
    const trim = { startMs: 3_000, endMs: 9_000 };
    // Played from before the start, after the end, or from the end itself: from the start.
    expect(previewAction('play', 0, trim)).toEqual({ kind: 'seek', ms: 3_000 });
    expect(previewAction('play', 11_000, trim)).toEqual({ kind: 'seek', ms: 3_000 });
    expect(previewAction('play', 8_990, trim)).toEqual({ kind: 'seek', ms: 3_000 });
    // Played from inside: carries on from there.
    expect(previewAction('play', 5_000, trim)).toEqual({ kind: 'none' });
    expect(previewAction('play', 3_000, trim)).toEqual({ kind: 'none' });
    // While playing: on until the end handle, stopped at it.
    expect(previewAction('frame', 8_999, trim)).toEqual({ kind: 'none' });
    expect(previewAction('frame', 9_000, trim)).toEqual({ kind: 'stop' });
    expect(previewAction('frame', 9_030, trim)).toEqual({ kind: 'stop' });
  });
});
