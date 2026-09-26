import { Observable } from 'rxjs';

import { AudioMixer } from './audio-mixer';
import { CaptureError } from './capture-error';
import {
  ChunkRecorder,
  DEFAULT_TIMESLICE_MS,
  DEFAULT_VIDEO_BITS_PER_SECOND,
  RecorderState,
  selectMimeType,
} from './chunk-recorder';
import { ChunkStore, TakeMeta } from './chunk-store';
import { LocksPort, persistTake } from './take-journal';

export interface TakeSessionOptions {
  display: MediaStream;
  mic: MediaStream | null;
  mixer: AudioMixer;
  store: ChunkStore;
  takeId: string;
  /** Test seams; default to the real browser APIs. */
  recorder?: ChunkRecorder;
  mimeType?: string | null;
  locks?: LocksPort | null;
  createStream?: (tracks: MediaStreamTrack[]) => MediaStream;
}

/**
 * One take, end to end on the device (§10 Record steps 4, 6–8): mix mic and display audio into
 * one track, record the display video plus that mix in 2 s chunks, and persist every chunk (and
 * the journal) before anything else sees it. Framework-free so the extension can reuse it.
 */
export class TakeSession {
  private constructor(
    readonly takeId: string,
    readonly mimeType: string,
    private readonly recorder: ChunkRecorder,
    /** Resolves with the final journal once the take has ended and every chunk is stored, whether
     * it ended by `stop()` or by the browser's "Stop sharing". */
    readonly ended: Promise<TakeMeta>,
  ) {}

  static async start(options: TakeSessionOptions): Promise<TakeSession> {
    const mimeType = options.mimeType === undefined ? selectMimeType() : options.mimeType;
    if (!mimeType) {
      throw new CaptureError('not-supported', 'this browser cannot record in a supported format');
    }
    const [video] = options.display.getVideoTracks();
    if (!video || video.readyState === 'ended') {
      throw new CaptureError('aborted', 'the shared screen is no longer available');
    }
    const audio = await options.mixer.mix({ mic: options.mic, display: options.display });
    const createStream = options.createStream ?? ((tracks) => new MediaStream(tracks));
    const stream = createStream(audio ? [video, audio] : [video]);

    const recorder = options.recorder ?? new ChunkRecorder();
    const persisted = await persistTake(recorder, options.store, {
      takeId: options.takeId,
      mimeType,
      locks: options.locks,
    });
    recorder.start(stream, {
      mimeType,
      timesliceMs: DEFAULT_TIMESLICE_MS,
      bitsPerSecond: DEFAULT_VIDEO_BITS_PER_SECOND,
    });
    return new TakeSession(options.takeId, mimeType, recorder, persisted.done);
  }

  get state$(): Observable<RecorderState> {
    return this.recorder.state$;
  }

  get state(): RecorderState {
    return this.recorder.state;
  }

  /** The pause-excluding recording timer. */
  elapsed$(intervalMs?: number): Observable<number> {
    return this.recorder.elapsed$(intervalMs);
  }

  pause(): void {
    this.recorder.pause();
  }

  resume(): void {
    this.recorder.resume();
  }

  /** Stops and waits until every chunk is stored; same result as `ended`. */
  async stop(): Promise<TakeMeta> {
    if (this.recorder.state === 'recording' || this.recorder.state === 'paused') {
      await this.recorder.stop();
    }
    return this.ended;
  }
}
