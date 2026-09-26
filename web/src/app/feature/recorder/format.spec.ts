import { formatClock, formatDuration, formatStart } from './format';

describe('formatDuration', () => {
  it.each([
    [0, '0 s'],
    [4_400, '4 s'],
    [45_000, '45 s'],
    [380_000, '6 min 20 s'],
    [3_720_000, '1 h 2 min'],
  ])('%i ms → %s', (ms, text) => {
    expect(formatDuration(ms)).toBe(text);
  });
});

describe('formatStart', () => {
  const now = new Date(2026, 8, 26, 16, 0);

  it('shows only the time for a recording started today', () => {
    expect(formatStart(new Date(2026, 8, 26, 14, 2).getTime(), now)).toMatch(/14.02|2.02/);
    expect(formatStart(new Date(2026, 8, 26, 14, 2).getTime(), now)).not.toMatch(/Sep/);
  });

  it('adds the date for an older recording', () => {
    expect(formatStart(new Date(2026, 8, 20, 9, 30).getTime(), now)).toMatch(/20/);
  });
});

describe('formatClock', () => {
  it.each([
    [0, '0:00'],
    [5_900, '0:05'],
    [723_000, '12:03'],
    [3_723_000, '1:02:03'],
  ])('%i ms → %s', (ms, text) => {
    expect(formatClock(ms)).toBe(text);
  });
});
