import { Observable, interval, map, startWith } from 'rxjs';

import { CaptureError } from './capture-error';

export type AudioSourceName = 'mic' | 'display';
export type LevelName = AudioSourceName | 'mix';
export type AudioLevels = Record<LevelName, number>;

export interface MixSources {
  mic?: MediaStream | null;
  display?: MediaStream | null;
}

/** The slice of `AudioContext` the mixer uses; injectable so tests can fake Web Audio. */
export type AudioContextPort = Pick<
  AudioContext,
  | 'createMediaStreamSource'
  | 'createMediaStreamDestination'
  | 'createAnalyser'
  | 'resume'
  | 'close'
  | 'state'
>;

const ANALYSER_FFT_SIZE = 1024;
const DEFAULT_LEVEL_INTERVAL_MS = 100;

/**
 * Mixes microphone and display (system/tab) audio into one track with Web Audio (§10 Record,
 * step 4: "mic + display audio → one `MediaStreamDestination` track"), and meters each input
 * and the mix. Framework-free (CLAUDE.md rule 9).
 */
export class AudioMixer {
  private context: AudioContextPort | null = null;
  private readonly nodes: AudioNode[] = [];
  private readonly analysers = new Map<LevelName, AnalyserNode>();
  private output: MediaStreamTrack | null = null;

  constructor(private readonly createContext: () => AudioContextPort = () => new AudioContext()) {}

  get track(): MediaStreamTrack | null {
    return this.output;
  }

  /**
   * Builds the mix from whichever inputs carry audio and returns the single mixed track, or
   * `null` when no input has audio (a silent recording is valid). Replaces any previous mix.
   * Call from a user gesture: the context is resumed here, and browsers only allow that then.
   */
  async mix(sources: MixSources): Promise<MediaStreamTrack | null> {
    this.teardown();
    const inputs = (['mic', 'display'] as const).filter(
      (name) => (sources[name]?.getAudioTracks().length ?? 0) > 0,
    );
    if (inputs.length === 0) {
      return null;
    }

    let context: AudioContextPort;
    try {
      context = this.context ?? this.createContext();
    } catch (error) {
      throw new CaptureError('not-supported', 'Web Audio is unavailable', { cause: error });
    }
    this.context = context;
    if (context.state === 'suspended') {
      await context.resume();
    }

    const destination = context.createMediaStreamDestination();
    const mixAnalyser = this.analyser(context, 'mix');
    for (const name of inputs) {
      // Non-null: `inputs` only holds names whose stream has audio tracks.
      const source = context.createMediaStreamSource(sources[name] as MediaStream);
      source.connect(destination);
      source.connect(this.analyser(context, name));
      source.connect(mixAnalyser);
      this.nodes.push(source);
    }
    this.nodes.push(destination);

    const [track] = destination.stream.getAudioTracks();
    this.output = track ?? null;
    return this.output;
  }

  /** RMS level of an input or the mix, 0 (silence) to 1 (full scale). 0 if not being mixed. */
  level(name: LevelName): number {
    const analyser = this.analysers.get(name);
    if (!analyser) {
      return 0;
    }
    const samples = new Float32Array(analyser.fftSize);
    analyser.getFloatTimeDomainData(samples);
    return rms(samples);
  }

  /** All levels, sampled every `intervalMs` (default 100 ms) while subscribed. */
  levels$(intervalMs = DEFAULT_LEVEL_INTERVAL_MS): Observable<AudioLevels> {
    return interval(intervalMs).pipe(
      startWith(0),
      map(() => ({
        mic: this.level('mic'),
        display: this.level('display'),
        mix: this.level('mix'),
      })),
    );
  }

  /** Disconnects the graph and closes the audio context. The mixed track ends. */
  async close(): Promise<void> {
    this.teardown();
    const context = this.context;
    this.context = null;
    if (context && context.state !== 'closed') {
      await context.close();
    }
  }

  private analyser(context: AudioContextPort, name: LevelName): AnalyserNode {
    const analyser = context.createAnalyser();
    analyser.fftSize = ANALYSER_FFT_SIZE;
    this.analysers.set(name, analyser);
    this.nodes.push(analyser);
    return analyser;
  }

  private teardown(): void {
    this.nodes.splice(0).forEach((node) => node.disconnect());
    this.analysers.clear();
    this.output?.stop();
    this.output = null;
  }
}

/** Root-mean-square of PCM samples in [-1, 1], clamped to [0, 1]. */
export function rms(samples: Float32Array): number {
  if (samples.length === 0) {
    return 0;
  }
  let sum = 0;
  for (const sample of samples) {
    sum += sample * sample;
  }
  return Math.min(1, Math.sqrt(sum / samples.length));
}
