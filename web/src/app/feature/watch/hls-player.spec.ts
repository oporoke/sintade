import { beforeEach, describe, expect, it, vi } from 'vitest';

type Handler = (event: string, data: unknown) => void;

const hls = vi.hoisted(() => {
  const state = {
    supported: true,
    instances: [] as FakeHls[],
  };
  class FakeHls {
    static Events = {
      MANIFEST_PARSED: 'manifest',
      LEVEL_SWITCHED: 'switched',
      ERROR: 'error',
    };
    static ErrorTypes = { NETWORK_ERROR: 'network', MEDIA_ERROR: 'media' };
    static isSupported = () => state.supported;
    handlers = new Map<string, Handler>();
    levels = [{ height: 360 }, { height: 1080 }, { height: 720 }];
    nextLevel = -1;
    loadSource = vi.fn();
    attachMedia = vi.fn();
    startLoad = vi.fn();
    recoverMediaError = vi.fn();
    destroy = vi.fn();
    constructor() {
      state.instances.push(this);
    }
    on(event: string, handler: Handler) {
      this.handlers.set(event, handler);
    }
    emit(event: string, data: unknown) {
      this.handlers.get(event)?.(event, data);
    }
  }
  return { state, FakeHls };
});

vi.mock('hls.js', () => ({ default: hls.FakeHls }));

import { VideoPlayer } from './hls-player';

const SOURCE = { hlsUrl: '/api/v1/s/abc/hls/master.m3u8', url: 'https://s/default.mp4?sig=1' };

function events() {
  return { levels: vi.fn(), switched: vi.fn(), fellBack: vi.fn() };
}

function vendor(name: string) {
  Object.defineProperty(navigator, 'vendor', { value: name, configurable: true });
}

function video(native = false): HTMLVideoElement {
  const element = document.createElement('video');
  element.canPlayType = (type: string) =>
    native && type === 'application/vnd.apple.mpegurl' ? 'maybe' : '';
  return element;
}

describe('VideoPlayer', () => {
  beforeEach(() => {
    vendor('Apple Computer, Inc.');
    hls.state.supported = true;
    hls.state.instances.length = 0;
  });

  it('plays the MP4 when there is no ladder yet', async () => {
    const element = video();
    const player = await VideoPlayer.start(element, { ...SOURCE, hlsUrl: null }, events());
    expect(player.playing).toBe('mp4');
    expect(element.getAttribute('src')).toBe(SOURCE.url);
    expect(hls.state.instances).toHaveLength(0);
  });

  it('hands the master playlist to the browser where it plays HLS itself', async () => {
    const element = video(true);
    const player = await VideoPlayer.start(element, SOURCE, events());
    expect(player.playing).toBe('native-hls');
    expect(element.getAttribute('src')).toBe(SOURCE.hlsUrl);
    expect(hls.state.instances).toHaveLength(0);
  });

  it('uses hls.js in Chromium even though it says it can play HLS', async () => {
    vendor('Google Inc.');
    const player = await VideoPlayer.start(video(true), SOURCE, events());
    expect(player.playing).toBe('hls');
  });

  it('falls back to the MP4 where neither HLS nor MSE exist', async () => {
    hls.state.supported = false;
    const element = video();
    const player = await VideoPlayer.start(element, SOURCE, events());
    expect(player.playing).toBe('mp4');
    expect(element.getAttribute('src')).toBe(SOURCE.url);
  });

  it('uses hls.js, reports the rungs lowest first and the one on screen', async () => {
    const element = video();
    const seen = events();
    const player = await VideoPlayer.start(element, SOURCE, seen);
    const [instance] = hls.state.instances;
    expect(player.playing).toBe('hls');
    expect(instance.loadSource).toHaveBeenCalledWith(SOURCE.hlsUrl);
    expect(instance.attachMedia).toHaveBeenCalledWith(element);

    instance.emit('manifest', { levels: instance.levels });
    expect(seen.levels).toHaveBeenCalledWith([
      { index: 0, height: 360 },
      { index: 2, height: 720 },
      { index: 1, height: 1080 },
    ]);
    instance.emit('switched', { level: 2 });
    expect(seen.switched).toHaveBeenCalledWith(720);
  });

  it('switches rung at the next fragment instead of flushing what is buffered', async () => {
    const player = await VideoPlayer.start(video(), SOURCE, events());
    const [instance] = hls.state.instances;
    player.setLevel(2);
    expect(instance.nextLevel).toBe(2);
    player.setLevel(-1);
    expect(instance.nextLevel).toBe(-1);
  });

  it('reads the playlists again once when segment URLs have expired, then falls back', async () => {
    const element = video();
    Object.defineProperty(element, 'currentTime', { value: 42, writable: true });
    const seen = events();
    await VideoPlayer.start(element, SOURCE, seen);
    const [instance] = hls.state.instances;
    const fatal = { fatal: true, type: 'network' };

    instance.emit('error', fatal);
    expect(instance.loadSource).toHaveBeenCalledTimes(2);
    expect(instance.startLoad).toHaveBeenCalledWith(42);
    expect(seen.fellBack).not.toHaveBeenCalled();

    instance.emit('error', fatal);
    expect(instance.destroy).toHaveBeenCalled();
    expect(element.getAttribute('src')).toBe(SOURCE.url);
    expect(seen.fellBack).toHaveBeenCalled();
  });

  it('recovers a media error once and ignores non-fatal ones', async () => {
    const seen = events();
    await VideoPlayer.start(video(), SOURCE, seen);
    const [instance] = hls.state.instances;
    instance.emit('error', { fatal: false, type: 'network' });
    expect(seen.fellBack).not.toHaveBeenCalled();
    instance.emit('error', { fatal: true, type: 'media' });
    expect(instance.recoverMediaError).toHaveBeenCalledTimes(1);
    instance.emit('error', { fatal: true, type: 'media' });
    expect(seen.fellBack).toHaveBeenCalled();
  });

  it('stops loading when destroyed', async () => {
    const player = await VideoPlayer.start(video(), SOURCE, events());
    player.destroy();
    expect(hls.state.instances[0].destroy).toHaveBeenCalled();
  });
});
