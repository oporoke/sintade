import { AudioLevels, AudioMixer, startAudio } from '../../capture';

export interface ToneResult {
  frequencyHz: number;
  role: 'mic' | 'display' | 'control';
  /** Estimated amplitude of this frequency in the recorded clip (0–1). */
  amplitude: number;
  present: boolean;
}

export interface MixSelfTestResult {
  /** Meter readings taken mid-clip. Theory: each 0.25-amplitude sine is RMS ≈ 0.177; the sum of
   * two unrelated sines is RMS ≈ 0.25. */
  levels: AudioLevels;
  mimeType: string;
  bytes: number;
  decodedSeconds: number;
  tones: ToneResult[];
}

/** Stand-in "mic" and "display" tones, plus a control frequency that must stay absent. */
const TONES = [
  { frequencyHz: 440, role: 'mic' },
  { frequencyHz: 1000, role: 'display' },
  { frequencyHz: 2500, role: 'control' },
] as const;
const TONE_GAIN = 0.25;
const PRESENT_THRESHOLD = 0.05;
const CLIP_MIME_TYPES = [
  'audio/webm;codecs=opus',
  'audio/webm',
  'audio/mp4',
  'audio/ogg;codecs=opus',
];

/**
 * Day 22 Check, "test clip contains both audio sources", without devices or permissions: two
 * synthetic tones stand in for the mic and display audio, go through the real `AudioMixer`,
 * get recorded by the browser's `MediaRecorder` into a clip, which is decoded and analysed.
 */
export async function runMixSelfTest(durationMs = 2000): Promise<MixSelfTestResult> {
  const sourceContext = new AudioContext();
  const mixer = new AudioMixer();
  try {
    await startAudio(sourceContext);
    const [mic, display] = [TONES[0], TONES[1]].map(({ frequencyHz }) => {
      const oscillator = sourceContext.createOscillator();
      oscillator.frequency.value = frequencyHz;
      const gain = sourceContext.createGain();
      gain.gain.value = TONE_GAIN;
      const destination = sourceContext.createMediaStreamDestination();
      oscillator.connect(gain).connect(destination);
      oscillator.start();
      return destination.stream;
    });

    const track = await mixer.mix({ mic, display });
    if (!track) {
      throw new Error('mixer produced no track');
    }
    const mimeType = CLIP_MIME_TYPES.find((type) => MediaRecorder.isTypeSupported(type)) ?? '';
    const recording = record(new MediaStream([track]), mimeType, durationMs);
    await delay(durationMs / 2);
    const levels = {
      mic: mixer.level('mic'),
      display: mixer.level('display'),
      mix: mixer.level('mix'),
    };
    const clip = await recording;

    const decodeContext = new AudioContext();
    try {
      const audio = await decodeContext.decodeAudioData(await clip.arrayBuffer());
      const samples = audio.getChannelData(0);
      // Skip the first and last quarter: encoder start-up and tail padding.
      const window = samples.subarray(
        Math.floor(samples.length / 4),
        Math.floor((samples.length * 3) / 4),
      );
      return {
        levels,
        mimeType: clip.type || mimeType,
        bytes: clip.size,
        decodedSeconds: audio.duration,
        tones: TONES.map(({ frequencyHz, role }) => {
          const amplitude = toneAmplitude(window, audio.sampleRate, frequencyHz);
          return { frequencyHz, role, amplitude, present: amplitude > PRESENT_THRESHOLD };
        }),
      };
    } finally {
      await decodeContext.close();
    }
  } finally {
    await mixer.close();
    await sourceContext.close();
  }
}

function delay(ms: number): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

function record(stream: MediaStream, mimeType: string, durationMs: number): Promise<Blob> {
  return new Promise((resolve, reject) => {
    const recorder = new MediaRecorder(stream, mimeType ? { mimeType } : undefined);
    const parts: Blob[] = [];
    recorder.ondataavailable = (event) => parts.push(event.data);
    recorder.onerror = () => reject(new Error('MediaRecorder failed'));
    recorder.onstop = () => resolve(new Blob(parts, { type: recorder.mimeType }));
    recorder.start();
    setTimeout(() => recorder.stop(), durationMs);
  });
}

/**
 * Amplitude of one frequency in a signal, via the Goertzel algorithm (a single DFT bin).
 * For a pure sine of amplitude A at `frequencyHz`, returns ≈ A.
 */
export function toneAmplitude(
  samples: Float32Array,
  sampleRate: number,
  frequencyHz: number,
): number {
  const n = samples.length;
  if (n === 0) {
    return 0;
  }
  const coefficient = 2 * Math.cos((2 * Math.PI * frequencyHz) / sampleRate);
  let previous = 0;
  let beforePrevious = 0;
  for (const sample of samples) {
    const current = sample + coefficient * previous - beforePrevious;
    beforePrevious = previous;
    previous = current;
  }
  const power =
    previous * previous + beforePrevious * beforePrevious - coefficient * previous * beforePrevious;
  return (2 * Math.sqrt(Math.max(0, power))) / n;
}
