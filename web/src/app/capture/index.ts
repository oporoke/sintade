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
export { CaptureError, toCaptureError } from './capture-error';
export {
  AUDIO_BITS_PER_SECOND,
  ChunkRecorder,
  DEFAULT_TIMESLICE_MS,
  DEFAULT_VIDEO_BITS_PER_SECOND,
  PREFERRED_MIME_TYPES,
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
