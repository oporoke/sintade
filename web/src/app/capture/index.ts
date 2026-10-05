/**
 * The capture engine's public surface. Framework-free: nothing under `capture/` may import
 * Angular (enforced by ESLint), so the Manifest V3 extension can reuse it unchanged.
 */
export { AUDIO_START_TIMEOUT_MS, AudioMixer, rms, startAudio } from './audio-mixer';
export type {
  AudioContextPort,
  AudioLevels,
  AudioSourceName,
  LevelName,
  MixSources,
} from './audio-mixer';
export {
  Compositor,
  DEFAULT_BUBBLE,
  DEFAULT_COMPOSITOR_FPS,
  MAX_BUBBLE_SIZE,
  MIN_BUBBLE_SIZE,
  bubbleRect,
  clampLayout,
  coverSquare,
} from './compositor';
export type { BubbleLayout, BubbleRect, BubbleShape, CompositorOptions } from './compositor';
export { CaptureError, toCaptureError } from './capture-error';
export {
  AUDIO_BITS_PER_SECOND,
  ChunkRecorder,
  DEFAULT_TIMER_INTERVAL_MS,
  DEFAULT_TIMESLICE_MS,
  DEFAULT_VIDEO_BITS_PER_SECOND,
  PREFERRED_MIME_TYPES,
  PREFERRED_VIDEO_ONLY_MIME_TYPES,
  STOP_TIMEOUT_MS,
  selectMimeType,
} from './chunk-recorder';
export type {
  Chunk,
  CreateMediaRecorder,
  RecorderOptions,
  RecorderState,
  RecordingSummary,
} from './chunk-recorder';
export type { CaptureErrorKind } from './capture-error';
export { SourceManager } from './source-manager';
export type { DisplayOptions, MediaDevicesPort, MicDevice } from './source-manager';
export { onTrackEnded } from './track-ended';
export {
  OpfsChunkStore,
  chunkName,
  openChunkStore,
  openIndexedDbChunkStore,
  openOpfsChunkStore,
  parseChunkName,
} from './chunk-store';
export type { ChunkStore, TakeMeta } from './chunk-store';
export { assembleTake, listOrphans, persistTake } from './take-journal';
export type {
  AssembledTake,
  LocksPort,
  OrphanTake,
  PersistOptions,
  PersistedTake,
} from './take-journal';
export { TakeSession } from './take-session';
export type { TakeSessionOptions } from './take-session';
export {
  BASE_BACKOFF_MS,
  MAX_BACKOFF_MS,
  PRESIGN_BATCH,
  PUT_TIMEOUT_MS,
  UploadError,
  UploadHttpError,
  Uploader,
  browserNetwork,
  fetchPut,
  isRetryable,
  sha256Hex,
} from './uploader';
export type {
  DigestChunk,
  FinalizedTake,
  NetworkPort,
  PresignedUrl,
  PutChunk,
  UploadApi,
  UploadProgress,
  UploadStatus,
  UploaderOptions,
  UploaderState,
} from './uploader';
export { uploadRecoveredTake } from './recovery';
export type { NewRecording, RecoverOptions, RecoveredUpload, RecoveryApi } from './recovery';
