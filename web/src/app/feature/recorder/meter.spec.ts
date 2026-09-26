import { meterValue } from './meter';

describe('meterValue', () => {
  it('jumps up to a louder reading', () => {
    expect(meterValue(0.1, 0.6)).toBe(0.6);
  });

  it('decays toward a quieter reading instead of dropping at once', () => {
    let shown = 0.8;
    const trail: number[] = [];
    for (let i = 0; i < 3; i += 1) {
      shown = meterValue(shown, 0);
      trail.push(Number(shown.toFixed(3)));
    }
    expect(trail).toEqual([0.68, 0.578, 0.491]);
  });

  it('never shows less than the current reading', () => {
    expect(meterValue(0.2, 0.3, 0.5)).toBe(0.3);
  });
});
