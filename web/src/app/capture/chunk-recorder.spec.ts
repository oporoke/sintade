import { toArray } from 'rxjs';

import {
  AUDIO_BITS_PER_SECOND,
  ChunkRecorder,
  CreateMediaRecorder,
  RecorderState,
  selectMimeType,
} from './chunk-recorder';

/** Mimics the browser: `stop()` flushes a final `dataavailable`, then fires `stop`. */
class FakeMediaRecorder extends EventTarget {
  state: RecordingState = 'inactive';
  timeslice: number | undefined;
  readonly mimeType: string;
  constructor(
    readonly stream: MediaStream,
    readonly options: MediaRecorderOptions,
  ) {
    super();
    this.mimeType = options.mimeType ?? '';
  }
  start(timeslice?: number): void {
    this.timeslice = timeslice;
    this.state = 'recording';
  }
  pause(): void {
    this.state = 'paused';
  }
  resume(): void {
    this.state = 'recording';
  }
  stop(): void {
    this.state = 'inactive';
    queueMicrotask(() => {
      this.emit(new Blob(['tail']));
      this.dispatchEvent(new Event('stop'));
    });
  }
  emit(data: Blob): void {
    const event = new Event('dataavailable') as Event & { data: Blob };
    event.data = data;
    this.dispatchEvent(event);
  }
  fail(): void {
    const event = new Event('error') as Event & { error: Error };
    const error = new Error('encoder died');
    error.name = 'UnknownError';
    event.error = error;
    this.dispatchEvent(event);
  }
}

class FakeTrack extends EventTarget {
  readonly kind = 'video';
  /** What the browser does when the user clicks "Stop sharing". */
  endFromBrowser(): void {
    this.dispatchEvent(new Event('ended'));
  }
}
const videoTrack = new FakeTrack();
const stream = { getVideoTracks: () => [videoTrack] } as unknown as MediaStream;
const options = {
  mimeType: 'video/webm;codecs=vp9,opus',
  timesliceMs: 2000,
  bitsPerSecond: 2_500_000,
};

function setup() {
  let fake: FakeMediaRecorder | undefined;
  const create: CreateMediaRecorder = (s, o) => {
    fake = new FakeMediaRecorder(s, o);
    return fake as unknown as MediaRecorder;
  };
  let clock = 1000;
  const recorder = new ChunkRecorder(create, () => clock);
  const states: RecorderState[] = [];
  recorder.state$.subscribe((state) => states.push(state));
  return {
    recorder,
    states,
    fake: () => fake as FakeMediaRecorder,
    advance: (ms: number) => (clock += ms),
  };
}

describe('selectMimeType', () => {
  it('prefers VP9, then VP8, then MP4', () => {
    expect(selectMimeType(() => true)).toBe('video/webm;codecs=vp9,opus');
    expect(selectMimeType((type) => !type.includes('vp9'))).toBe('video/webm;codecs=vp8,opus');
    expect(selectMimeType((type) => type.startsWith('video/mp4'))).toBe(
      'video/mp4;codecs=avc1,mp4a',
    );
  });

  it('is null when nothing is supported', () => {
    expect(selectMimeType(() => false)).toBeNull();
  });
});

describe('ChunkRecorder', () => {
  it('starts with the timeslice, MIME type and bitrates', () => {
    const { recorder, fake } = setup();
    recorder.start(stream, options);

    expect(fake().timeslice).toBe(2000);
    expect(fake().stream).toBe(stream);
    expect(fake().options).toEqual({
      mimeType: 'video/webm;codecs=vp9,opus',
      videoBitsPerSecond: 2_500_000,
      audioBitsPerSecond: AUDIO_BITS_PER_SECOND,
    });
    expect(recorder.state).toBe('recording');
    expect(recorder.mimeType).toBe('video/webm;codecs=vp9,opus');
  });

  it('emits non-empty chunks with consecutive indexes, including the final flush', async () => {
    const { recorder, fake, advance } = setup();
    const chunks: { index: number; size: number }[] = [];
    const done = new Promise<void>((resolve) =>
      recorder.chunks$.pipe(toArray()).subscribe((all) => {
        chunks.push(...all.map(({ index, blob }) => ({ index, size: blob.size })));
        resolve();
      }),
    );

    recorder.start(stream, options);
    fake().emit(new Blob(['header+frames']));
    fake().emit(new Blob([])); // browsers can hand over empty slices; they're skipped
    fake().emit(new Blob(['frames']));
    advance(6000);
    const summary = await recorder.stop();
    await done;

    expect(chunks.map((chunk) => chunk.index)).toEqual([0, 1, 2]);
    expect(chunks.every((chunk) => chunk.size > 0)).toBe(true);
    expect(summary).toEqual({ chunkCount: 3, durationMs: 6000 });
  });

  it('moves through idle → recording → stopping → idle', async () => {
    const { recorder, states } = setup();
    recorder.start(stream, options);
    await recorder.stop();
    expect(states).toEqual(['idle', 'recording', 'stopping', 'idle']);
  });

  it('refuses a second start (one instance per take)', async () => {
    const { recorder } = setup();
    recorder.start(stream, options);
    await recorder.stop();
    expect(() => recorder.start(stream, options)).toThrow(/one per take/);
  });

  it('rejects stop when not recording', async () => {
    const { recorder } = setup();
    await expect(recorder.stop()).rejects.toMatchObject({ kind: 'aborted' });
  });

  it('maps a constructor failure (e.g. unsupported MIME) to a CaptureError', () => {
    const recorder = new ChunkRecorder(() => {
      const error = new Error('bad type');
      error.name = 'NotSupportedError';
      throw error;
    });
    expect(() => recorder.start(stream, options)).toThrow(
      expect.objectContaining({ kind: 'not-supported' }),
    );
    expect(recorder.state).toBe('idle');
  });

  it('surfaces a recorder error on chunks$ and returns to idle', () => {
    const { recorder, fake } = setup();
    const errors: unknown[] = [];
    recorder.chunks$.subscribe({ error: (error: unknown) => errors.push(error) });
    recorder.start(stream, options);
    fake().fail();

    expect(errors).toHaveLength(1);
    expect(errors[0]).toMatchObject({ name: 'CaptureError', kind: 'unknown' });
    expect(recorder.state).toBe('idle');
  });

  describe('pause and resume (Day 24)', () => {
    it('pauses and resumes the underlying recorder with matching states', () => {
      const { recorder, fake, states } = setup();
      recorder.start(stream, options);
      recorder.pause();
      expect(fake().state).toBe('paused');
      recorder.resume();
      expect(fake().state).toBe('recording');
      expect(states).toEqual(['idle', 'recording', 'paused', 'recording']);
    });

    it('excludes paused time from the timer and the final duration', async () => {
      const { recorder, advance } = setup();
      recorder.start(stream, options);
      advance(2000);
      recorder.pause();
      advance(5000);
      expect(recorder.elapsedMs()).toBe(2000); // frozen while paused
      recorder.resume();
      advance(1500);
      expect(recorder.elapsedMs()).toBe(3500);

      await expect(recorder.stop()).resolves.toMatchObject({ durationMs: 3500 });
    });

    it('can stop while paused, not counting the pause', async () => {
      const { recorder, advance } = setup();
      recorder.start(stream, options);
      advance(1000);
      recorder.pause();
      advance(9000);
      await expect(recorder.stop()).resolves.toMatchObject({ durationMs: 1000 });
    });

    it('rejects pause/resume in the wrong state', () => {
      const { recorder } = setup();
      expect(() => recorder.pause()).toThrow(/cannot pause while idle/);
      recorder.start(stream, options);
      expect(() => recorder.resume()).toThrow(/cannot resume while recording/);
    });

    it('elapsed$ ticks the pause-excluding timer', async () => {
      vi.useFakeTimers();
      try {
        const { recorder, advance } = setup();
        const ticks: number[] = [];
        recorder.start(stream, options);
        const subscription = recorder.elapsed$(250).subscribe((ms) => ticks.push(ms));
        advance(250);
        await vi.advanceTimersByTimeAsync(250);
        subscription.unsubscribe();
        expect(ticks).toEqual([0, 250]);
      } finally {
        vi.useRealTimers();
      }
    });
  });

  describe('source track ending (Day 24)', () => {
    it('stops by itself when the video track ends and reports it on stopped$', async () => {
      const { recorder, advance } = setup();
      const stopped = new Promise((resolve) => recorder.stopped$.subscribe(resolve));
      recorder.start(stream, options);
      advance(3000);
      videoTrack.endFromBrowser();

      await expect(stopped).resolves.toEqual({ chunkCount: 1, durationMs: 3000 });
      expect(recorder.state).toBe('idle');
    });

    it('also stops when the track ends during a pause', async () => {
      const { recorder } = setup();
      const stopped = new Promise((resolve) => recorder.stopped$.subscribe(resolve));
      recorder.start(stream, options);
      recorder.pause();
      videoTrack.endFromBrowser();
      await expect(stopped).resolves.toMatchObject({ chunkCount: 1 });
    });

    it('stopped$ also fires for a manual stop, and a second stop() shares the result', async () => {
      const { recorder } = setup();
      const onStopped = vi.fn();
      recorder.stopped$.subscribe(onStopped);
      recorder.start(stream, options);
      const first = recorder.stop();
      const second = recorder.stop();
      expect(second).toBe(first);
      await first;
      expect(onStopped).toHaveBeenCalledTimes(1);
    });

    it('ignores the track ending after the take is over', async () => {
      const { recorder } = setup();
      recorder.start(stream, options);
      await recorder.stop();
      expect(() => videoTrack.endFromBrowser()).not.toThrow();
      expect(recorder.state).toBe('idle');
    });
  });
});
