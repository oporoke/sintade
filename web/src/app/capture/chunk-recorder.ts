import { BehaviorSubject, Observable, Subject } from 'rxjs';

import { CaptureError, toCaptureError } from './capture-error';

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
 * Framework-free (CLAUDE.md rule 9).
 */
export class ChunkRecorder {
  private recorder: MediaRecorder | null = null;
  private nextIndex = 0;
  private startedAt = 0;
  private readonly chunksSubject = new Subject<Chunk>();
  private readonly stateSubject = new BehaviorSubject<RecorderState>('idle');

  readonly chunks$: Observable<Chunk> = this.chunksSubject.asObservable();
  readonly state$: Observable<RecorderState> = this.stateSubject.asObservable();

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
      this.chunksSubject.error(toCaptureError(error));
      this.stateSubject.next('idle');
    });
    this.recorder = recorder;
    recorder.start(options.timesliceMs);
    this.startedAt = this.now();
    this.stateSubject.next('recording');
  }

  /** Stops, waits for the final chunk, and completes `chunks$`. */
  stop(): Promise<RecordingSummary> {
    const recorder = this.recorder;
    if (!recorder || this.state === 'idle' || this.state === 'stopping') {
      return Promise.reject(new CaptureError('aborted', 'not recording'));
    }
    const durationMs = this.now() - this.startedAt;
    this.stateSubject.next('stopping');
    return new Promise((resolve) => {
      // The spec fires the last `dataavailable` before `stop`, so every chunk is in by then.
      recorder.addEventListener(
        'stop',
        () => {
          this.stateSubject.next('idle');
          this.chunksSubject.complete();
          resolve({ chunkCount: this.nextIndex, durationMs });
        },
        { once: true },
      );
      recorder.stop();
    });
  }
}
