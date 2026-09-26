/**
 * The capture engine's public surface. Framework-free: nothing under `capture/` may import
 * Angular (enforced by ESLint), so the Manifest V3 extension can reuse it unchanged.
 */
export { AudioMixer, rms } from './audio-mixer';
export type {
  AudioContextPort,
  AudioLevels,
  AudioSourceName,
  LevelName,
  MixSources,
} from './audio-mixer';
export { CaptureError, toCaptureError } from './capture-error';
export type { CaptureErrorKind } from './capture-error';
export { SourceManager } from './source-manager';
export type { DisplayOptions, MediaDevicesPort, MicDevice } from './source-manager';
