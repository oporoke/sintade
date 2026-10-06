import {
  chapterAt,
  formatChapterLines,
  formatClock,
  parseChapterLines,
  parseClock,
} from './chapters-text';

describe('clock', () => {
  it('formats and parses', () => {
    expect(formatClock(0)).toBe('0:00');
    expect(formatClock(65_000)).toBe('1:05');
    expect(formatClock(3_725_000)).toBe('1:02:05');
    expect(parseClock('1:05')).toBe(65_000);
    expect(parseClock('1:02:05')).toBe(3_725_000);
    expect(parseClock('90')).toBe(90_000);
  });

  it('refuses things that are not times', () => {
    for (const bad of ['', 'a', '1:60', '1:2:3:4', '-1', '1.5']) {
      expect(parseClock(bad), bad).toBeNull();
    }
  });
});

describe('parseChapterLines', () => {
  it('reads one chapter per line and skips blanks', () => {
    expect(parseChapterLines('0:00 Intro\n\n 1:05   Demo time \n')).toEqual({
      ok: true,
      chapters: [
        { start_ms: 0, title: 'Intro' },
        { start_ms: 65_000, title: 'Demo time' },
      ],
    });
  });

  it('names the bad line', () => {
    expect(parseChapterLines('0:00 Intro\nnope Title')).toEqual({ ok: false, line: 2 });
    expect(parseChapterLines('0:30')).toEqual({ ok: false, line: 1 });
  });

  it('round-trips', () => {
    const chapters = [
      { start_ms: 0, title: 'Intro' },
      { start_ms: 3_725_000, title: 'Late' },
    ];
    expect(parseChapterLines(formatChapterLines(chapters))).toEqual({ ok: true, chapters });
  });
});

describe('chapterAt', () => {
  const chapters = [
    { start_ms: 0, title: 'a' },
    { start_ms: 10_000, title: 'b' },
  ];
  it('is the last chapter that has started', () => {
    expect(chapterAt(chapters, 0)?.title).toBe('a');
    expect(chapterAt(chapters, 9.9)?.title).toBe('a');
    expect(chapterAt(chapters, 10)?.title).toBe('b');
    expect(chapterAt([{ start_ms: 5000, title: 'x' }], 1)).toBeNull();
  });
});
