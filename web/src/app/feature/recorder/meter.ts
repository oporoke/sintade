/**
 * Peak-hold with decay for the device-check meter: a new reading replaces the shown value only
 * if it's louder; otherwise the shown value falls by `decay` per tick. Short sounds (a clap, a
 * plosive) then stay visible for a moment instead of flickering past a 50–100 ms sample.
 */
export function meterValue(shown: number, reading: number, decay = 0.85): number {
  return Math.max(reading, shown * decay);
}
