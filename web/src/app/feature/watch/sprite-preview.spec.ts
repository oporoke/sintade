import { cueAt, parseSpriteVtt } from './sprite-preview';

const VTT = `WEBVTT

00:00:00.000 --> 00:00:02.000
https://s/sheet0.jpg?sig=a#xywh=0,0,160,90

00:00:02.000 --> 00:00:04.000
https://s/sheet0.jpg?sig=a#xywh=160,0,160,90

00:00:04.000 --> 00:01:05.500
https://s/sheet1.jpg?sig=b#xywh=0,90,160,90
`;

describe('parseSpriteVtt', () => {
  it('reads times, the signed sheet (query kept) and the tile rectangle', () => {
    const cues = parseSpriteVtt(VTT);
    expect(cues).toHaveLength(3);
    expect(cues[1]).toEqual({
      startS: 2,
      endS: 4,
      url: 'https://s/sheet0.jpg?sig=a',
      x: 160,
      y: 0,
      width: 160,
      height: 90,
    });
    expect(cues[2].endS).toBe(65.5);
  });

  it('skips cues it cannot read', () => {
    expect(parseSpriteVtt('WEBVTT\n\nnope --> x\nfoo#xywh=a,b,c,d\n')).toEqual([]);
    expect(parseSpriteVtt('')).toEqual([]);
  });
});

describe('cueAt', () => {
  const cues = parseSpriteVtt(VTT);

  it('finds the cue that covers a time', () => {
    expect(cueAt(cues, 0)?.x).toBe(0);
    expect(cueAt(cues, 1.99)?.x).toBe(0);
    expect(cueAt(cues, 2)?.x).toBe(160);
    expect(cueAt(cues, 30)?.y).toBe(90);
  });

  it('uses the last cue past the end and none when there are none', () => {
    expect(cueAt(cues, 999)?.y).toBe(90);
    expect(cueAt([], 3)).toBeNull();
  });
});
