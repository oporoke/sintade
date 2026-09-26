import { toneAmplitude } from './mix-self-test';

describe('toneAmplitude (Goertzel)', () => {
  const sampleRate = 48_000;
  const sine = (frequency: number, amplitude: number, length = 24_000) =>
    Float32Array.from(
      { length },
      (_, i) => amplitude * Math.sin((2 * Math.PI * frequency * i) / sampleRate),
    );

  it('recovers the amplitude of a pure tone', () => {
    expect(toneAmplitude(sine(440, 0.25), sampleRate, 440)).toBeCloseTo(0.25, 2);
  });

  it('finds both tones in a mix and nothing at an absent frequency', () => {
    const a = sine(440, 0.25);
    const b = sine(1000, 0.25);
    const mix = a.map((value, i) => value + b[i]);
    expect(toneAmplitude(mix, sampleRate, 440)).toBeCloseTo(0.25, 2);
    expect(toneAmplitude(mix, sampleRate, 1000)).toBeCloseTo(0.25, 2);
    expect(toneAmplitude(mix, sampleRate, 2500)).toBeLessThan(0.01);
  });

  it('is 0 for no samples', () => {
    expect(toneAmplitude(new Float32Array(0), sampleRate, 440)).toBe(0);
  });
});
