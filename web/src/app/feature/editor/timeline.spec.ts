import { SpriteCue } from '../watch/sprite-preview';
import { fractionToMs, msToFraction, thumbnailCount, thumbnailStrip } from './timeline';

/** One cue a second, as `GenerateSprite` makes them, for a recording of `seconds`. */
function cues(seconds: number): SpriteCue[] {
  return Array.from({ length: seconds }, (_, i) => ({
    startS: i,
    endS: i + 1,
    url: 'sheet.jpg',
    x: (i % 10) * 160,
    y: Math.floor(i / 10) * 90,
    width: 160,
    height: 90,
  }));
}

describe('timeline', () => {
  // Day 93 Check: the timeline matches the video's length, end to end.
  it('spans exactly the recording: its edges are 0 and the length', () => {
    expect(msToFraction(0, 12_345)).toBe(0);
    expect(msToFraction(12_345, 12_345)).toBe(1);
    expect(msToFraction(6_172.5, 12_345)).toBe(0.5);
    expect(fractionToMs(0, 12_345)).toBe(0);
    expect(fractionToMs(1, 12_345)).toBe(12_345);
    // Past either edge is the edge.
    expect(fractionToMs(1.2, 12_345)).toBe(12_345);
    expect(fractionToMs(-0.1, 12_345)).toBe(0);
    expect(msToFraction(20_000, 12_345)).toBe(1);
    expect(msToFraction(5, 0)).toBe(0);
  });

  it('lays thumbnails edge to edge across the whole recording', () => {
    const strip = thumbnailStrip(cues(12), 12_000, 8);
    expect(strip).toHaveLength(8);
    expect(strip[0].startMs).toBe(0);
    expect(strip[7].endMs).toBe(12_000);
    for (let i = 1; i < strip.length; i++) {
      expect(strip[i].startMs).toBe(strip[i - 1].endMs);
    }
    // Each shows the frame at its middle: the first ~0.75 s, the last ~11.25 s.
    expect(strip[0].cue.startS).toBe(0);
    expect(strip[7].cue.startS).toBe(11);
  });

  it('has no thumbnails without a sprite', () => {
    expect(thumbnailStrip([], 12_000, 8)).toEqual([]);
    expect(thumbnailCount(800, 54, undefined)).toBe(0);
  });

  it('fits as many thumbnails as their shape allows', () => {
    // 16:9 at 54 px tall is 96 px wide: 800 px holds 8.
    expect(thumbnailCount(800, 54, cues(1)[0])).toBe(8);
    expect(thumbnailCount(30, 54, cues(1)[0])).toBe(1);
  });
});
