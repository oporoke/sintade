/**
 * The editor timeline's waveform (ADR-0039): draws the peaks JSON the `GenerateWaveform` job
 * stores. Framework-free so it can be timed and reused anywhere.
 *
 * Drawing costs one bar per pixel column at most, whatever the recording's length: the peaks
 * (up to 20,000) are first folded to the canvas width, keeping each column's loudest.
 */

/** `img/waveform.json`, as the API's signed URL serves it. */
export interface Waveform {
  version: number;
  duration_ms: number;
  /** How long each peak covers. */
  bucket_ms: number;
  /** `false` for a recording without sound: `peaks` is then empty. */
  has_audio: boolean;
  /** The loudest sample of each bucket, 0–255. */
  peaks: number[];
}

/** What drawing needs of a 2D context (a real one, or a stand-in in tests). */
export type WaveformContext = Pick<
  CanvasRenderingContext2D,
  'canvas' | 'clearRect' | 'fillRect' | 'fillStyle'
>;

/** The loudest peak under each of `columns` pixel columns (stretched when there are fewer). */
export function columnPeaks(peaks: readonly number[], columns: number): Uint8Array {
  const out = new Uint8Array(Math.max(0, Math.floor(columns)));
  if (peaks.length === 0) {
    return out;
  }
  for (let column = 0; column < out.length; column++) {
    const start = Math.floor((column * peaks.length) / out.length);
    const end = Math.max(start + 1, Math.floor(((column + 1) * peaks.length) / out.length));
    let loudest = 0;
    for (let i = start; i < end; i++) {
      if (peaks[i] > loudest) {
        loudest = peaks[i];
      }
    }
    out[column] = loudest;
  }
  return out;
}

/**
 * Draws `waveform` across the whole canvas: bars mirrored around the middle, scaled so the
 * loudest reaches the edges, and at least a 1 px line so silence still shows the timeline.
 */
export function drawWaveform(ctx: WaveformContext, waveform: Waveform, color: string): void {
  const { width, height } = ctx.canvas;
  ctx.clearRect(0, 0, width, height);
  ctx.fillStyle = color;
  const middle = height / 2;
  if (!waveform.has_audio || waveform.peaks.length === 0) {
    ctx.fillRect(0, Math.floor(middle), width, 1);
    return;
  }
  const columns = columnPeaks(waveform.peaks, width);
  let loudest = 1;
  for (const peak of columns) {
    loudest = Math.max(loudest, peak);
  }
  for (let x = 0; x < columns.length; x++) {
    const bar = Math.max(1, Math.round((columns[x] / loudest) * height));
    ctx.fillRect(x, Math.round(middle - bar / 2), 1, bar);
  }
}
