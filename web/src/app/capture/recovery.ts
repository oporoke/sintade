import { ChunkStore } from './chunk-store';
import {
  FinalizedTake,
  UploadApi,
  UploadError,
  UploadHttpError,
  UploadProgress,
  Uploader,
  UploaderOptions,
} from './uploader';

/** What creating a recording needs (`POST /recordings`), for takes recorded offline. */
export interface NewRecording {
  mime_type: string;
  has_system_audio: boolean;
  has_mic: boolean;
  has_camera: boolean;
}

export interface RecoveryApi extends UploadApi {
  createRecording(body: NewRecording): Promise<{ recording_id: string; take_id: string }>;
}

export interface RecoveredUpload extends FinalizedTake {
  /** The server take the chunks now belong to. */
  take_id: string;
  chunkCount: number;
}

export interface RecoverOptions {
  api: RecoveryApi;
  store: ChunkStore;
  /** The stored take (its key in the chunk store). */
  takeId: string;
  onProgress?: (progress: UploadProgress) => void;
  /** Test seams passed to the `Uploader`. */
  uploader?: Partial<Pick<UploaderOptions, 'put' | 'digest' | 'network' | 'delay' | 'random'>>;
}

/**
 * Uploads a take left behind by a closed or crashed tab (§10 Recover): the contiguous chunks
 * from index 0 (what `assembleTake` would play), then finalize, then clear it from the device.
 * If the take is on the server, only what the server lacks is uploaded (`GET /status`). If it
 * was recorded while the server was unreachable, a recording is created for it first, and the
 * new take id is journaled before anything is uploaded, so an interrupted recovery resumes the
 * same take instead of creating another.
 */
export async function uploadRecoveredTake(options: RecoverOptions): Promise<RecoveredUpload> {
  const { api, store, takeId } = options;
  const [meta, indexes] = await Promise.all([store.getMeta(takeId), store.indexes(takeId)]);
  let chunkCount = 0;
  while (indexes.includes(chunkCount)) {
    chunkCount += 1;
  }
  if (chunkCount === 0) {
    throw new Error('this recording has no usable parts to upload');
  }
  const mimeType = meta?.mimeType || 'video/webm';

  let serverTakeId = meta?.serverTakeId;
  if (!serverTakeId) {
    const created = await api.createRecording({
      mime_type: mimeType,
      has_system_audio: false,
      // The journal doesn't record the sources; an audio codec means some audio was mixed in.
      has_mic: /opus|mp4a|vorbis/.test(mimeType),
      has_camera: false,
    });
    serverTakeId = created.take_id;
    await store.putMeta({
      ...(meta ?? { takeId, startedAt: Date.now(), mimeType, chunkCount, durationMs: 0 }),
      serverTakeId,
    });
  }

  const uploader = new Uploader({
    ...options.uploader,
    api,
    store,
    takeId: serverTakeId,
    storeTakeId: takeId,
  });
  const subscription = options.onProgress && uploader.progress$.subscribe(options.onProgress);
  try {
    await uploader.resume(chunkCount);
    const finalized = await uploader.finalize(chunkCount, meta?.durationMs ?? 0);
    return { ...finalized, take_id: serverTakeId, chunkCount };
  } catch (error) {
    // The server abandoned the take after a day without uploads (`SweepStaleUploads`), so it
    // no longer accepts it (409). The chunks are still here: upload them as a new recording.
    if (meta?.serverTakeId && isRecordingClosed(error)) {
      const detached = { ...meta };
      delete detached.serverTakeId;
      await store.putMeta(detached);
      return uploadRecoveredTake(options);
    }
    throw error;
  } finally {
    subscription?.unsubscribe();
  }
}

function isRecordingClosed(error: unknown): boolean {
  const reason = error instanceof UploadError ? error.reason : error;
  return reason instanceof UploadHttpError && reason.source === 'api' && reason.status === 409;
}
