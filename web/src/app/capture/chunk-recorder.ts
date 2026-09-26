import { BehaviorSubject, Observable, Subject, interval, map, startWith } from 'rxjs';

import { CaptureError, toCaptureError } from './capture-error';
import { onTrackEnded } from './track-ended';

export type RecorderState = 'idle' | 'recording' | 'paused' | 'stopping';

export interface Chunk {
  index: number;
  blob: Blob;
}

export interface RecorderOptions {
  mimeType: string;
  /** How often the browser hands over a chunk. The design records in 2 s slices (§10). */
  timesliceMs: number;
  /** Video bitrate. Audio is fixed at `AUDIO_BITS_PER_SECOND`. */
  bitsPerSecond: number;
}

export interface RecordingSummary {
  chunkCount: number;
  durationMs: number;
}

/** In order of preference (§10 Record): VP9 WebM, VP8 WebM, then H.264/AAC MP4 for Safari. */
export const PREFERRED_MIME_TYPES = [
  'video/webm;codecs=vp9,opus',
  'video/webm;codecs=vp8,opus',
  'video/mp4;codecs=avc1,mp4a',
] as const;
export const DEFAULT_TIMESLICE_MS = 2000;
export const DEFAULT_VIDEO_BITS_PER_SECOND = 2_500_000;
export const AUDIO_BITS_PER_SECOND = 128_000;
export const DEFAULT_TIMER_INTERVAL_MS = 250;

/** The first preferred MIME type this browser can record, or `null` if none. */
export function selectMimeType(
  isTypeSupported: (type: string) => boolean = (type) =>
    typeof MediaRecorder !== 'undefined' && MediaRecorder.isTypeSupported(type),
): string | null {
  return PREFERRED_MIME_TYPES.find((type) => isTypeSupported(type)) ?? null;
}

export type CreateMediaRecorder = (
  stream: MediaStream,
  options: MediaRecorderOptions,
) => MediaRecorder;

/**
 * Records one take as a sequence of timesliced chunks (§5 contract). The chunks are one
 * continuous stream: concatenated in index order they form the playable file, which is how the
 * worker rebuilds it (§10 Process, step 2). One instance per take; `chunks$` completes on stop.
 *
 * The recording stops by itself when the stream's video track ends, e.g. the user clicks the
 * browser's "Stop sharing" (§10 Record, step 8); `stopped$` reports either kind of stop.
 * Durations and the timer exclude paused time. Framework-free (CLAUDE.md rule 9).
 */
export class ChunkRecorder {
  private recorder: MediaRecorder | null = null;
  private nextIndex = 0;
  private startedAt = 0;
  private pausedTotalMs = 0;
  private pausedAt: number | null = null;
  private stopping: Promise<RecordingSummary> | null = null;
  private unsubscribeEnded: (() => void) | null = null;
  private readonly chunksSubject = new Subject<Chunk>();
  private readonly stateSubject = new BehaviorSubject<RecorderState>('idle');
  private readonly stoppedSubject = new Subject<RecordingSummary>();

  readonly chunks$: Observable<Chunk> = this.chunksSubject.asObservable();
  readonly state$: Observable<RecorderState> = this.stateSubject.asObservable();
  /** Emits once when the take ends, whether by `stop()` or by the source track ending. */
  readonly stopped$: Observable<RecordingSummary> = this.stoppedSubject.asObservable();

  constructor(
    private readonly createRecorder: CreateMediaRecorder = (stream, options) =>
      new MediaRecorder(stream, options),
    private readonly now: () => number = () => performance.now(),
  ) {}

  get state(): RecorderState {
    return this.stateSubject.value;
  }

  get mimeType(): string | null {
    return this.recorder?.mimeType ?? null;
  }

  /** Recorded time so far, excluding pauses. */
  elapsedMs(): number {
    if (!this.recorder) {
      return 0;
    }
    const until = this.pausedAt ?? this.now();
    return until - this.startedAt - this.pausedTotalMs;
  }

  /** The recording timer (excludes pauses), ticking every `intervalMs` while subscribed. */
  elapsed$(intervalMs = DEFAULT_TIMER_INTERVAL_MS): Observable<number> {
    return interval(intervalMs).pipe(
      startWith(0),
      map(() => this.elapsedMs()),
    );
  }

  start(stream: MediaStream, options: RecorderOptions): void {
    if (this.state !== 'idle' || this.recorder) {
      throw new CaptureError('aborted', 'this recorder has already been started; use one per take');
    }
    let recorder: MediaRecorder;
    try {
      recorder = this.createRecorder(stream, {
        mimeType: options.mimeType,
        videoBitsPerSecond: options.bitsPerSecond,
        audioBitsPerSecond: AUDIO_BITS_PER_SECOND,
      });
    } catch (error) {
      throw toCaptureError(error);
    }
    recorder.addEventListener('dataavailable', (event) => {
      const { data } = event as BlobEvent;
      if (data.size > 0) {
        this.chunksSubject.next({ index: this.nextIndex++, blob: data });
      }
    });
    recorder.addEventListener('error', (event) => {
      const error = (event as ErrorEvent).error ?? event;
      this.detachEndedListener();
      this.chunksSubject.error(toCaptureError(error));
      this.stateSubject.next('idle');
    });
    this.recorder = recorder;
    recorder.start(options.timesliceMs);
    this.startedAt = this.now();
    this.stateSubject.next('recording');

    const [video] = stream.getVideoTracks();
    if (video) {
      this.unsubscribeEnded = onTrackEnded(video, () => {
        if (this.state === 'recording' || this.state === 'paused') {
          void this.stop();
        }
      });
    }
  }

  pause(): void {
    if (this.state !== 'recording' || !this.recorder) {
      throw new CaptureError('aborted', `cannot pause while ${this.state}`);
    }
    this.recorder.pause();
    this.pausedAt = this.now();
    this.stateSubject.next('paused');
  }

  resume(): void {
    if (this.state !== 'paused' || !this.recorder || this.pausedAt === null) {
      throw new CaptureError('aborted', `cannot resume while ${this.state}`);
    }
    this.recorder.resume();
    this.pausedTotalMs += this.now() - this.pausedAt;
    this.pausedAt = null;
    this.stateSubject.next('recording');
  }

  /**
   * Stops (from recording or paused), waits for the final chunk, and completes `chunks$`.
   * Calling it again while stopping returns the same result.
   */
  stop(): Promise<RecordingSummary> {
    if (this.stopping) {
      return this.stopping;
    }
    const recorder = this.recorder;
    if (!recorder || (this.state !== 'recording' && this.state !== 'paused')) {
      return Promise.reject(new CaptureError('aborted', 'not recording'));
    }
    const durationMs = this.elapsedMs();
    this.detachEndedListener();
    this.stateSubject.next('stopping');
    this.stopping = new Promise((resolve) => {
      // The spec fires the last `dataavailable` before `stop`, so every chunk is in by then.
      recorder.addEventListener(
        'stop',
        () => {
          const summary = { chunkCount: this.nextIndex, durationMs };
          this.stateSubject.next('idle');
          this.chunksSubject.complete();
          this.stoppedSubject.next(summary);
          this.stoppedSubject.complete();
          resolve(summary);
        },
        { once: true },
      );
      recorder.stop();
    });
    return this.stopping;
  }

  private detachEndedListener(): void {
    this.unsubscribeEnded?.();
    this.unsubscribeEnded = null;
  }
}
