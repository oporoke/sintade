import { firstValueFrom, take, toArray } from 'rxjs';

import { CaptureError, toCaptureError } from './capture-error';
import { MediaDevicesPort, SourceManager } from './source-manager';

class FakeTrack extends EventTarget {
  readonly stop = vi.fn();
  constructor(readonly kind: 'audio' | 'video') {
    super();
  }
  /** What the browser does when the user clicks "Stop sharing". */
  endFromBrowser(): void {
    this.dispatchEvent(new Event('ended'));
  }
}

function fakeStream(...kinds: ('audio' | 'video')[]) {
  const tracks = kinds.map((kind) => new FakeTrack(kind));
  const stream = {
    getTracks: () => tracks,
    getVideoTracks: () => tracks.filter((track) => track.kind === 'video'),
  } as unknown as MediaStream;
  return { stream, tracks };
}

function device(kind: MediaDeviceKind, deviceId: string, label = ''): MediaDeviceInfo {
  return { kind, deviceId, label, groupId: 'g', toJSON: () => ({}) } as MediaDeviceInfo;
}

class FakeMediaDevices extends EventTarget {
  getDisplayMedia = vi.fn();
  getUserMedia = vi.fn();
  enumerateDevices = vi.fn<() => Promise<MediaDeviceInfo[]>>().mockResolvedValue([]);
}

function domError(name: string): Error {
  const error = new Error(`${name} happened`);
  error.name = name;
  return error;
}

function setup() {
  const devices = new FakeMediaDevices();
  const manager = new SourceManager(devices as unknown as MediaDevicesPort);
  return { devices, manager };
}

describe('SourceManager', () => {
  describe('pickDisplay', () => {
    it('asks for 30 fps video and no audio by default', async () => {
      const { devices, manager } = setup();
      const { stream } = fakeStream('video');
      devices.getDisplayMedia.mockResolvedValue(stream);

      await expect(manager.pickDisplay({ systemAudio: false })).resolves.toBe(stream);
      expect(devices.getDisplayMedia).toHaveBeenCalledWith({
        video: { frameRate: 30 },
        audio: false,
      });
      expect(manager.currentDisplay).toBe(stream);
    });

    it('requests system audio when asked', async () => {
      const { devices, manager } = setup();
      devices.getDisplayMedia.mockResolvedValue(fakeStream('video', 'audio').stream);

      await manager.pickDisplay({ systemAudio: true, frameRate: 60 });
      expect(devices.getDisplayMedia).toHaveBeenCalledWith({
        video: { frameRate: 60 },
        audio: true,
      });
    });

    it('stops the previous display when a new one is picked', async () => {
      const { devices, manager } = setup();
      const first = fakeStream('video');
      devices.getDisplayMedia.mockResolvedValueOnce(first.stream);
      devices.getDisplayMedia.mockResolvedValueOnce(fakeStream('video').stream);

      await manager.pickDisplay({ systemAudio: false });
      await manager.pickDisplay({ systemAudio: false });
      expect(first.tracks[0].stop).toHaveBeenCalled();
    });

    it('emits displayEnded$ when the browser ends the share, and forgets the stream', async () => {
      const { devices, manager } = setup();
      const { stream, tracks } = fakeStream('video');
      devices.getDisplayMedia.mockResolvedValue(stream);
      const ended = vi.fn();
      manager.displayEnded$.subscribe(ended);

      await manager.pickDisplay({ systemAudio: false });
      tracks[0].endFromBrowser();

      expect(ended).toHaveBeenCalledTimes(1);
      expect(manager.currentDisplay).toBeNull();
    });

    it('maps a cancelled or denied picker to permission-denied', async () => {
      const { devices, manager } = setup();
      devices.getDisplayMedia.mockRejectedValue(domError('NotAllowedError'));

      await expect(manager.pickDisplay({ systemAudio: false })).rejects.toMatchObject({
        name: 'CaptureError',
        kind: 'permission-denied',
      });
      expect(manager.currentDisplay).toBeNull();
    });

    it('is not-supported when the API is missing', async () => {
      const manager = new SourceManager(undefined);
      await expect(manager.pickDisplay({ systemAudio: false })).rejects.toMatchObject({
        kind: 'not-supported',
      });
    });
  });

  describe('openMic', () => {
    it('opens the default mic without video', async () => {
      const { devices, manager } = setup();
      const { stream } = fakeStream('audio');
      devices.getUserMedia.mockResolvedValue(stream);

      await expect(manager.openMic()).resolves.toBe(stream);
      expect(devices.getUserMedia).toHaveBeenCalledWith({ audio: true, video: false });
    });

    it('pins a chosen device exactly and replaces the previous mic', async () => {
      const { devices, manager } = setup();
      const first = fakeStream('audio');
      devices.getUserMedia.mockResolvedValueOnce(first.stream);
      devices.getUserMedia.mockResolvedValueOnce(fakeStream('audio').stream);

      await manager.openMic();
      await manager.openMic('usb-mic');
      expect(devices.getUserMedia).toHaveBeenLastCalledWith({
        audio: { deviceId: { exact: 'usb-mic' } },
        video: false,
      });
      expect(first.tracks[0].stop).toHaveBeenCalled();
    });

    it('maps an unplugged device to no-device and a busy one to device-busy', async () => {
      const { devices, manager } = setup();
      devices.getUserMedia.mockRejectedValueOnce(domError('OverconstrainedError'));
      devices.getUserMedia.mockRejectedValueOnce(domError('NotReadableError'));

      await expect(manager.openMic('gone')).rejects.toMatchObject({ kind: 'no-device' });
      await expect(manager.openMic()).rejects.toMatchObject({ kind: 'device-busy' });
    });
  });

  describe('microphones', () => {
    it('lists audio inputs only, with fallback labels before permission', async () => {
      const { devices, manager } = setup();
      devices.enumerateDevices.mockResolvedValue([
        device('videoinput', 'cam'),
        device('audioinput', 'default'),
        device('audioinput', 'usb', 'USB Mic'),
        device('audiooutput', 'speakers'),
      ]);

      await expect(manager.listMics()).resolves.toEqual([
        { deviceId: 'default', label: 'Microphone 1' },
        { deviceId: 'usb', label: 'USB Mic' },
      ]);
    });

    it('mics$ re-emits on devicechange and stops listening on unsubscribe', async () => {
      const { devices, manager } = setup();
      devices.enumerateDevices
        .mockResolvedValueOnce([device('audioinput', 'a', 'A')])
        .mockResolvedValueOnce([device('audioinput', 'a', 'A'), device('audioinput', 'b', 'B')]);

      const emissions = firstValueFrom(manager.mics$.pipe(take(2), toArray()));
      await Promise.resolve();
      devices.dispatchEvent(new Event('devicechange'));

      expect((await emissions).map((mics) => mics.map((mic) => mic.deviceId))).toEqual([
        ['a'],
        ['a', 'b'],
      ]);
      devices.dispatchEvent(new Event('devicechange'));
      expect(devices.enumerateDevices).toHaveBeenCalledTimes(2);
    });
  });

  it('stopAll stops every track and clears both sources', async () => {
    const { devices, manager } = setup();
    const display = fakeStream('video', 'audio');
    const mic = fakeStream('audio');
    devices.getDisplayMedia.mockResolvedValue(display.stream);
    devices.getUserMedia.mockResolvedValue(mic.stream);
    await manager.pickDisplay({ systemAudio: true });
    await manager.openMic();

    manager.stopAll();

    [...display.tracks, ...mic.tracks].forEach((track) => expect(track.stop).toHaveBeenCalled());
    expect(manager.currentDisplay).toBeNull();
    expect(manager.currentMic).toBeNull();
  });
});

describe('toCaptureError', () => {
  it('passes CaptureErrors through and defaults unknown names to unknown', () => {
    const original = new CaptureError('aborted', 'x');
    expect(toCaptureError(original)).toBe(original);
    expect(toCaptureError(domError('WeirdError')).kind).toBe('unknown');
    expect(toCaptureError('string failure').kind).toBe('unknown');
  });
});
