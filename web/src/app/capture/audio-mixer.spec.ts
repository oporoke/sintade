import { firstValueFrom } from 'rxjs';

import { AudioContextPort, AudioMixer, rms } from './audio-mixer';

class FakeNode {
  readonly connections: FakeNode[] = [];
  readonly disconnect = vi.fn();
  connect(node: FakeNode): FakeNode {
    this.connections.push(node);
    return node;
  }
}

class FakeAnalyser extends FakeNode {
  fftSize = 2048;
  constructor(private readonly value: number) {
    super();
  }
  getFloatTimeDomainData(samples: Float32Array): void {
    samples.fill(this.value);
  }
}

class FakeDestination extends FakeNode {
  readonly track = { kind: 'audio', stop: vi.fn() } as unknown as MediaStreamTrack;
  readonly stream = { getAudioTracks: () => [this.track] } as unknown as MediaStream;
}

class FakeSource extends FakeNode {
  constructor(readonly input: MediaStream) {
    super();
  }
}

class FakeContext {
  state: AudioContextState = 'suspended';
  readonly sources: FakeSource[] = [];
  readonly destinations: FakeDestination[] = [];
  readonly analysers: FakeAnalyser[] = [];
  /** Constant sample value each successive analyser reports: mic 0.5, display 0.25, mix 0.75. */
  private readonly analyserValues: number[];

  constructor(analyserValues: number[] = []) {
    this.analyserValues = [...analyserValues];
  }

  resume = vi.fn(async () => {
    this.state = 'running';
  });
  close = vi.fn(async () => {
    this.state = 'closed';
  });
  createMediaStreamSource = vi.fn((stream: MediaStream) => {
    const source = new FakeSource(stream);
    this.sources.push(source);
    return source;
  });
  createMediaStreamDestination = vi.fn(() => {
    const destination = new FakeDestination();
    this.destinations.push(destination);
    return destination;
  });
  createAnalyser = vi.fn(() => {
    const analyser = new FakeAnalyser(this.analyserValues.shift() ?? 0);
    this.analysers.push(analyser);
    return analyser;
  });
}

function stream(audioTracks: number): MediaStream {
  return {
    getAudioTracks: () => Array.from({ length: audioTracks }, () => ({ kind: 'audio' })),
  } as unknown as MediaStream;
}

function setup(analyserValues?: number[]) {
  const context = new FakeContext(analyserValues);
  const create = vi.fn(() => context as unknown as AudioContextPort);
  return { context, create, mixer: new AudioMixer(create) };
}

describe('AudioMixer', () => {
  it('routes mic and display audio into one destination track', async () => {
    const { context, mixer } = setup();
    const mic = stream(1);
    const display = stream(1);

    const track = await mixer.mix({ mic, display });

    expect(context.destinations).toHaveLength(1);
    const destination = context.destinations[0];
    expect(track).toBe(destination.track);
    expect(context.sources.map((source) => source.input)).toEqual([mic, display]);
    for (const source of context.sources) {
      expect(source.connections).toContain(destination);
    }
  });

  it('resumes a suspended context (autoplay policy)', async () => {
    const { context, mixer } = setup();
    await mixer.mix({ mic: stream(1) });
    expect(context.resume).toHaveBeenCalled();
    expect(context.state).toBe('running');
  });

  it('mixes only inputs that carry audio', async () => {
    const { context, mixer } = setup();
    const mic = stream(1);
    await mixer.mix({ mic, display: stream(0) });
    expect(context.sources.map((source) => source.input)).toEqual([mic]);
  });

  it('returns null and creates no context when nothing has audio', async () => {
    const { create, mixer } = setup();
    await expect(mixer.mix({ mic: null, display: stream(0) })).resolves.toBeNull();
    expect(create).not.toHaveBeenCalled();
  });

  it('meters each input and the mix separately', async () => {
    // Analysers are created in order: mix first, then mic, then display.
    const { mixer } = setup([0.75, 0.5, 0.25]);
    await mixer.mix({ mic: stream(1), display: stream(1) });

    expect(mixer.level('mix')).toBeCloseTo(0.75);
    expect(mixer.level('mic')).toBeCloseTo(0.5);
    expect(mixer.level('display')).toBeCloseTo(0.25);
    await expect(firstValueFrom(mixer.levels$(10))).resolves.toEqual({
      mic: expect.closeTo(0.5),
      display: expect.closeTo(0.25),
      mix: expect.closeTo(0.75),
    });
  });

  it('reports 0 for inputs that are not being mixed', async () => {
    const { mixer } = setup([0.75, 0.5]);
    await mixer.mix({ mic: stream(1) });
    expect(mixer.level('display')).toBe(0);
  });

  it('re-mixing tears down the previous graph and ends its track', async () => {
    const { context, mixer } = setup();
    const first = await mixer.mix({ mic: stream(1) });
    const firstNodes = [...context.sources, ...context.analysers, ...context.destinations];

    await mixer.mix({ mic: stream(1), display: stream(1) });

    firstNodes.forEach((node) => expect(node.disconnect).toHaveBeenCalled());
    expect(first?.stop).toHaveBeenCalled();
  });

  it('close() disconnects everything and closes the context once', async () => {
    const { context, mixer } = setup();
    await mixer.mix({ mic: stream(1) });
    await mixer.close();
    await mixer.close();

    expect(context.close).toHaveBeenCalledTimes(1);
    expect(mixer.track).toBeNull();
    expect(mixer.level('mic')).toBe(0);
  });

  it('is not-supported when Web Audio cannot be created', async () => {
    const mixer = new AudioMixer(() => {
      throw new ReferenceError('AudioContext is not defined');
    });
    await expect(mixer.mix({ mic: stream(1) })).rejects.toMatchObject({ kind: 'not-supported' });
  });
});

describe('rms', () => {
  it('is 0 for silence and empty input, amplitude for a constant signal', () => {
    expect(rms(new Float32Array(0))).toBe(0);
    expect(rms(new Float32Array(8))).toBe(0);
    expect(rms(new Float32Array(8).fill(-0.5))).toBeCloseTo(0.5);
  });

  it('is 1/√2 of the peak for a full-scale sine', () => {
    const sine = Float32Array.from({ length: 1000 }, (_, i) => Math.sin((2 * Math.PI * i) / 100));
    expect(rms(sine)).toBeCloseTo(Math.SQRT1_2, 3);
  });
});
