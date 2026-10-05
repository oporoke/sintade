import { Observable } from 'rxjs';

import { AudioMixer } from './audio-mixer';
import { BubbleLayout, Compositor } from './compositor';
import { CaptureError } from './capture-error';
import {
  ChunkRecorder,
  DEFAULT_TIMESLICE_MS,
  DEFAULT_VIDEO_BITS_PER_SECOND,
  RecorderState,
  selectMimeType,
} from './chunk-recorder';
import { ChunkStore, TakeMeta } from './chunk-store';
import { onTrackEnded } from './track-ended';
import { QualityPreset, bitsPerSecond } from './quality';
import { LocksPort, persistTake } from './take-journal';

export interface TakeSessionOptions {
  display: MediaStream;
  mic: MediaStream | null;
  mixer: AudioMixer;
  /** The webcam; when set, the recorded video is the screen with the camera bubble on top. */
  camera?: MediaStream | null;
  /** Resolution/frame rate/bitrate; defaults to the recorder's standard 30 fps rate. */
  quality?: QualityPreset;
  /** Test seam for building the compositor. */
  createCompositor?: (screen: MediaStream, camera: MediaStream) => Compositor;
  store: ChunkStore;
  /** The key the chunks are stored under: the server's take id when the take is uploaded. */
  takeId: string;
  /** Set when `takeId` is a take on the server (journaled, so recovery can resume it). */
  serverTakeId?: string;
  /** Test seams; default to the real browser APIs. */
  recorder?: ChunkRecorder;
  mimeType?: string | null;
  isTypeSupported?: (type: string) => boolean;
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
    /** Each chunk's index once it is durably stored, for the uploader. */
    readonly stored$: Observable<number>,
    private readonly compositor: Compositor | null = null,
  ) {}

  static async start(options: TakeSessionOptions): Promise<TakeSession> {
    const [displayVideo] = options.display.getVideoTracks();
    if (!displayVideo || displayVideo.readyState === 'ended') {
      throw new CaptureError('aborted', 'the shared screen is no longer available');
    }
    // The compositor exists only when a camera is on; otherwise the screen track goes straight
    // to the recorder (§10 Record).
    const compositor = options.camera
      ? (
          options.createCompositor ??
          ((screen, camera) => new Compositor({ screen, camera, fps: options.quality?.fps }))
        )(options.display, options.camera)
      : null;
    const video = compositor?.track ?? displayVideo;
    const audio = await options.mixer.mix({ mic: options.mic, display: options.display });
    // Chosen after mixing: a video-only take needs a MIME type without an audio codec.
    const mimeType =
      options.mimeType === undefined
        ? selectMimeType(options.isTypeSupported, { audio: audio !== null })
        : options.mimeType;
    if (!mimeType) {
      compositor?.stop();
      await options.mixer.close();
      throw new CaptureError('not-supported', 'this browser cannot record in a supported format');
    }
    const createStream = options.createStream ?? ((tracks) => new MediaStream(tracks));
    const stream = createStream(audio ? [video, audio] : [video]);

    const recorder = options.recorder ?? new ChunkRecorder();
    const persisted = await persistTake(recorder, options.store, {
      takeId: options.takeId,
      mimeType,
      locks: options.locks,
      serverTakeId: options.serverTakeId,
    });
    recorder.start(stream, {
      mimeType,
      timesliceMs: DEFAULT_TIMESLICE_MS,
      bitsPerSecond: options.quality
        ? bitsPerSecond(options.quality)
        : DEFAULT_VIDEO_BITS_PER_SECOND,
    });
    const session = new TakeSession(
      options.takeId,
      mimeType,
      recorder,
      persisted.done,
      persisted.stored$,
      compositor,
    );
    if (compositor) {
      // The recorder watches the canvas track, which never ends by itself: end the take when
      // the shared screen ends, and release the compositor once the take is stored.
      onTrackEnded(displayVideo, () => void session.stop().catch(() => undefined));
      const release = () => compositor.stop();
      persisted.done.then(release, release);
    }
    return session;
  }

  /** The webcam bubble's layout, or null when there is no camera. */
  get bubble(): BubbleLayout | null {
    return this.compositor?.bubble ?? null;
  }

  /** Moves, resizes or reshapes the bubble (or switches camera-only) while recording. */
  setBubble(layout: Partial<BubbleLayout>): void {
    this.compositor?.setBubble(layout);
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
