/** Output heights offered by the recorder, tallest last. */
export const RESOLUTIONS = [720, 1080, 2160] as const;
export type Resolution = (typeof RESOLUTIONS)[number];
export const FRAME_RATES = [30, 60] as const;
export type FrameRate = (typeof FRAME_RATES)[number];

export interface QualityPreset {
  /** Height in pixels (720, 1080, 2160 = 4K). */
  height: Resolution;
  fps: FrameRate;
}

export const DEFAULT_QUALITY: QualityPreset = { height: 1080, fps: 30 };

/** Heights at 30 fps; 60 fps uses 1.6x (motion needs more bits, not twice as many). */
const BITS_PER_SECOND_30: Record<Resolution, number> = {
  720: 3_000_000,
  1080: 5_000_000,
  2160: 16_000_000,
};
const FPS_60_FACTOR = 1.6;

export function bitsPerSecond(preset: QualityPreset): number {
  const base = BITS_PER_SECOND_30[preset.height];
  return Math.round(preset.fps === 60 ? base * FPS_60_FACTOR : base);
}

/** The resolutions a plan allows (`maxResolution` is its tallest, in pixels of height). */
export function allowedResolutions(maxResolution: number): Resolution[] {
  const allowed = RESOLUTIONS.filter((height) => height <= maxResolution);
  // A plan below 720p still gets the smallest preset rather than none.
  return allowed.length > 0 ? allowed : [RESOLUTIONS[0]];
}

/** Brings a preset within the plan: a too-tall request drops to the tallest allowed height. */
export function clampQuality(preset: QualityPreset, maxResolution: number): QualityPreset {
  const allowed = allowedResolutions(maxResolution);
  const height = allowed.includes(preset.height) ? preset.height : allowed[allowed.length - 1];
  return { ...preset, height };
}

/** The microphone's browser-side processing (Day 74 toggles). All on by default, as browsers do. */
export interface AudioProcessing {
  noiseSuppression: boolean;
  echoCancellation: boolean;
  autoGainControl: boolean;
}

export const DEFAULT_AUDIO_PROCESSING: AudioProcessing = {
  noiseSuppression: true,
  echoCancellation: true,
  autoGainControl: true,
};
