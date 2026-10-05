import type { ChunkStore, TakeMeta, TakeSession, TakeSessionOptions, Uploader } from '@capture';
import { UploadHttpError, selectMimeType } from '@capture';

import type { RecordingErrorCode, RecordingState } from '../shared/recording';
import { IDLE } from '../shared/recording';
import type { CreatedRecording, NewRecordingBody } from './upload-api';

/** Stop this long before the plan's limit so finalize accepts the take (as the web recorder does). */
export const LIMIT_MARGIN_MS = 1500;

export interface ControllerApi {
  createRecording(body: NewRecordingBody): Promise<CreatedRecording>;
  createLink(recordingId: string): Promise<{ slug: string }>;
}

export interface ControllerDeps {
  api: ControllerApi;
  /** The captured tab: video, and its audio if the tab makes sound. */
  stream: MediaStream;
  store: ChunkStore;
  mixer: TakeSessionOptions['mixer'];
  startTake: (options: TakeSessionOptions) => Promise<TakeSession>;
  createUploader: (takeId: string) => Pick<Uploader, 'enqueue' | 'finalize'>;
  /** The public address of a recording's link. */
  linkUrl: (slug: string) => string;
  onState: (state: RecordingState) => void;
  now?: () => number;
  /** Test seam; defaults to the browser's format choice. */
  mimeType?: (hasAudio: boolean) => string | null;
}

/**
 * One tab recording, start to link (docs/design.md §10 Record, from the extension): create the
 * recording on the server, record the tab in 2 s chunks through the shared capture engine,
 * upload every chunk as it is stored, and when the take ends (Stop, the tab closing, or the
 * plan's limit) finalize it and create its share link. Framework-free and driven entirely by
 * its dependencies, so it is tested without a browser.
 */
export class RecordingController {
  private session: TakeSession | null = null;
  private state: RecordingState = IDLE;
  private readonly subscriptions: { unsubscribe(): void }[] = [];

  constructor(private readonly deps: ControllerDeps) {}

  get current(): RecordingState {
    return this.state;
  }

  async start(): Promise<void> {
    const { deps } = this;
    this.set({ phase: 'starting' });
    const hasAudio = deps.stream.getAudioTracks().length > 0;
    const mimeType = (deps.mimeType ?? ((audio) => selectMimeType(undefined, { audio })))(hasAudio);
    if (!mimeType) {
      return this.fail('capture-failed', 'This browser cannot record in a supported format.');
    }

    let recording: CreatedRecording;
    try {
      recording = await deps.api.createRecording({
        mime_type: mimeType,
        has_system_audio: hasAudio,
        has_mic: false,
        has_camera: false,
      });
    } catch (error) {
      return this.failCreate(error);
    }

    try {
      this.session = await deps.startTake({
        display: deps.stream,
        mic: null,
        mixer: deps.mixer,
        store: deps.store,
        takeId: recording.take_id,
        serverTakeId: recording.take_id,
        mimeType,
      });
    } catch (error) {
      return this.fail('capture-failed', error instanceof Error ? error.message : String(error));
    }
    const session = this.session;
    const uploader = deps.createUploader(recording.take_id);
    const now = deps.now ?? Date.now;
    this.set({
      phase: 'recording',
      startedAt: now(),
      maxDurationMs: recording.max_duration_ms,
    });

    this.subscriptions.push(
      session.stored$.subscribe({ next: (idx) => uploader.enqueue(idx), error: () => undefined }),
      session.elapsed$(500).subscribe((ms) => {
        if (ms >= recording.max_duration_ms - LIMIT_MARGIN_MS) {
          void this.stop();
        }
      }),
    );
    // Ends by Stop, the tab closing or the plan's limit alike.
    session.ended.then(
      (take) => this.finish(take, recording, uploader),
      (error: unknown) =>
        this.fail('capture-failed', error instanceof Error ? error.message : String(error)),
    );
  }

  async stop(): Promise<void> {
    await this.session?.stop().catch(() => undefined);
  }

  private async finish(
    take: TakeMeta,
    recording: CreatedRecording,
    uploader: Pick<Uploader, 'finalize'>,
  ): Promise<void> {
    this.unsubscribe();
    this.deps.stream.getTracks().forEach((track) => track.stop());
    this.set({ phase: 'uploading' });
    try {
      if (take.chunkCount === 0) {
        throw new Error('nothing was recorded');
      }
      await uploader.finalize(take.chunkCount, take.durationMs);
    } catch {
      return this.fail(
        'upload-failed',
        "The recording couldn't be uploaded. Check your connection and try again.",
      );
    }
    try {
      const link = await this.deps.api.createLink(recording.recording_id);
      this.set({
        phase: 'done',
        recordingId: recording.recording_id,
        url: this.deps.linkUrl(link.slug),
      });
    } catch {
      // Uploaded and processing; only the link is missing. The library has the recording.
      this.fail(
        'upload-failed',
        'The recording is uploaded, but its link could not be created. Find it in your library.',
      );
    }
  }

  private failCreate(error: unknown): void {
    if (error instanceof UploadHttpError) {
      if (error.status === 401) {
        return this.fail('not-signed-in', 'Sign in to Sintade to record.');
      }
      if (error.status === 402) {
        return this.fail(
          'limit-reached',
          "You've reached your plan's limit on recordings. Delete one to record another.",
        );
      }
      if (error.status === 0) {
        return this.fail('unreachable', "Can't reach Sintade. Check your connection.");
      }
    }
    this.fail('unreachable', "Sintade couldn't start the recording. Try again.");
  }

  private fail(code: RecordingErrorCode, message: string): void {
    this.unsubscribe();
    this.deps.stream.getTracks().forEach((track) => track.stop());
    this.set({ phase: 'error', code, message });
  }

  private unsubscribe(): void {
    this.subscriptions.splice(0).forEach((subscription) => subscription.unsubscribe());
  }

  private set(state: RecordingState): void {
    this.state = state;
    this.deps.onState(state);
  }
}
