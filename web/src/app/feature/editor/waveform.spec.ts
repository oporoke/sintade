import { columnPeaks, drawWaveform, Waveform, WaveformContext } from './waveform';

/** A stand-in 2D context that records the bars drawn. */
function recorder(width: number, height: number) {
  const bars: [number, number, number, number][] = [];
  const ctx: WaveformContext = {
    canvas: { width, height } as HTMLCanvasElement,
    fillStyle: '',
    clearRect: () => undefined,
    fillRect: (x: number, y: number, w: number, h: number) => {
      bars.push([x, y, w, h]);
    },
  };
  return { ctx, bars };
}

function waveform(peaks: number[], hasAudio = true): Waveform {
  return {
    version: 1,
    duration_ms: peaks.length * 50,
    bucket_ms: 50,
    has_audio: hasAudio,
    peaks,
  };
}

describe('waveform', () => {
  it('keeps the loudest peak under each column', () => {
    expect(Array.from(columnPeaks([1, 9, 3, 4, 0, 2], 3))).toEqual([9, 4, 2]);
    // Fewer peaks than columns: each is stretched.
    expect(Array.from(columnPeaks([5, 7], 4))).toEqual([5, 5, 7, 7]);
    expect(Array.from(columnPeaks([], 3))).toEqual([0, 0, 0]);
  });

  it('draws bars mirrored around the middle, the loudest edge to edge', () => {
    const { ctx, bars } = recorder(4, 100);
    drawWaveform(ctx, waveform([255, 0, 128, 255]), '#000');
    expect(bars).toEqual([
      [0, 0, 1, 100],
      [1, 50, 1, 1],
      [2, 25, 1, 50],
      [3, 0, 1, 100],
    ]);
  });

  it('draws a flat line for a recording without sound', () => {
    const { ctx, bars } = recorder(300, 40);
    drawWaveform(ctx, waveform([], false), '#000');
    expect(bars).toEqual([[0, 20, 300, 1]]);
  });

  // Day 92 Check: the largest waveform (20,000 peaks: a recording of 16 min 40 s or longer)
  // renders in under 200 ms, one bar per pixel column.
  it('renders the largest waveform in under 200 ms', () => {
    const peaks = Array.from({ length: 20_000 }, (_, i) => (i * 37) % 256);
    const { ctx, bars } = recorder(1_600, 80);
    const started = performance.now();
    drawWaveform(ctx, waveform(peaks), '#000');
    const elapsed = performance.now() - started;
    expect(bars.length).toBe(1_600);
    expect(elapsed).toBeLessThan(200);
  });
});
