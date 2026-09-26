import { AudioMixer } from './audio-mixer';
import { ChunkRecorder, CreateMediaRecorder } from './chunk-recorder';
import { ChunkStore, TakeMeta } from './chunk-store';
import { TakeSession } from './take-session';

class FakeMediaRecorder extends EventTarget {
  state: RecordingState = 'inactive';
  readonly mimeType = 'video/webm;codecs=vp9,opus';
  constructor(readonly stream: MediaStream) {
    super();
  }
  start() {
    this.state = 'recording';
  }
  pause() {
    this.state = 'paused';
  }
  resume() {
    this.state = 'recording';
  }
  stop() {
    this.state = 'inactive';
    queueMicrotask(() => {
      const event = new Event('dataavailable') as Event & { data: Blob };
      event.data = new Blob(['last']);
      this.dispatchEvent(event);
      this.dispatchEvent(new Event('stop'));
    });
  }
}

class MemoryStore {
  readonly backend = 'indexeddb';
  readonly chunks = new Map<number, Blob>();
  meta: TakeMeta | null = null;
  put = vi.fn(async (_id: string, index: number, blob: Blob) => void this.chunks.set(index, blob));
  putMeta = vi.fn(async (meta: TakeMeta) => void (this.meta = { ...meta }));
}

class FakeVideoTrack extends EventTarget {
  readonly kind = 'video';
  readyState: MediaStreamTrackState = 'live';
}

function setup(options: { audio?: boolean; mimeType?: string | null } = {}) {
  const video = new FakeVideoTrack();
  const display = { getVideoTracks: () => [video] } as unknown as MediaStream;
  const mixed = { kind: 'audio' } as MediaStreamTrack;
  const mixer = { mix: vi.fn().mockResolvedValue(options.audio === false ? null : mixed) };
  let recorded: MediaStream | undefined;
  const create: CreateMediaRecorder = (stream) => {
    recorded = stream;
    return new FakeMediaRecorder(stream) as unknown as MediaRecorder;
  };
  const store = new MemoryStore();
  const createStream = (tracks: MediaStreamTrack[]) =>
    ({
      tracks,
      getVideoTracks: () => tracks.filter((t) => t.kind === 'video'),
    }) as unknown as MediaStream;
  const start = () =>
    TakeSession.start({
      display,
      mic: null,
      mixer: mixer as unknown as AudioMixer,
      store: store as unknown as ChunkStore,
      takeId: '0190a1b2-c3d4-7e5f-8a9b-0c1d2e3f4a5b',
      recorder: new ChunkRecorder(create),
      mimeType: options.mimeType === undefined ? 'video/webm;codecs=vp9,opus' : options.mimeType,
      locks: null,
      createStream,
    });
  return {
    start,
    mixer,
    store,
    video,
    recorded: () => recorded as unknown as { tracks: MediaStreamTrack[] },
  };
}

describe('TakeSession', () => {
  it('records the display video plus the mixed audio track', async () => {
    const { start, mixer, recorded, video } = setup();
    const session = await start();

    expect(mixer.mix).toHaveBeenCalledWith({ mic: null, display: expect.anything() });
    expect(recorded().tracks.map((track) => track.kind)).toEqual(['video', 'audio']);
    expect(recorded().tracks[0]).toBe(video);
    expect(session.state).toBe('recording');
  });

  it('records video only when there is no audio to mix', async () => {
    const { start, recorded } = setup({ audio: false });
    await start();
    expect(recorded().tracks.map((track) => track.kind)).toEqual(['video']);
  });

  it('pauses, resumes, and on stop resolves once every chunk is stored', async () => {
    const { start, store } = setup();
    const session = await start();
    session.pause();
    expect(session.state).toBe('paused');
    session.resume();

    const meta = await session.stop();
    expect(store.chunks.size).toBe(1);
    expect(meta).toMatchObject({ chunkCount: 1, mimeType: 'video/webm;codecs=vp9,opus' });
    await expect(session.ended).resolves.toEqual(meta);
    expect(session.state).toBe('idle');
  });

  it('ends on its own when the browser stops the share', async () => {
    const { start, video } = setup();
    const session = await start();
    video.dispatchEvent(new Event('ended'));
    await expect(session.ended).resolves.toMatchObject({ chunkCount: 1 });
  });

  it('refuses to start without a supported format or a live screen', async () => {
    await expect(setup({ mimeType: null }).start()).rejects.toMatchObject({
      kind: 'not-supported',
    });
    const ended = setup();
    ended.video.readyState = 'ended';
    await expect(ended.start()).rejects.toMatchObject({ kind: 'aborted' });
  });
});
