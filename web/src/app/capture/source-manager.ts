import { Observable, Subject } from 'rxjs';

import { CaptureError, toCaptureError } from './capture-error';
import { AudioProcessing, QualityPreset } from './quality';
import { onTrackEnded } from './track-ended';

/** The slice of `MediaDevices` the source manager uses; injectable so tests can fake it. */
export type MediaDevicesPort = Pick<
  MediaDevices,
  | 'getDisplayMedia'
  | 'getUserMedia'
  | 'enumerateDevices'
  | 'addEventListener'
  | 'removeEventListener'
>;

export interface MicDevice {
  /** Empty before permission in Firefox: the device can't be addressed individually yet. */
  deviceId: string;
  /** Browsers hide real labels until mic permission is granted; a numbered fallback is used. */
  label: string;
}

export interface DisplayOptions {
  /** Defaults to 30, the design's recording frame rate (§10 Record, step 3). */
  frameRate?: number;
  /** Ideal capture height; the browser may deliver less (the shared surface's own size). */
  height?: number;
  /** Draw the mouse pointer into the capture (default) or leave it out. Chromium honours it;
   * other browsers ignore the hint. It applies when the screen is picked. */
  cursor?: 'always' | 'never';
  /** Ask for tab/system audio. Only Chromium-family browsers deliver it (see CapabilityService). */
  systemAudio: boolean;
}

const DEFAULT_FRAME_RATE = 30;

/**
 * Acquires and releases the raw capture sources: the display (screen, window or tab) and the
 * microphone. Framework-free (CLAUDE.md rule 9) so the browser extension can reuse it.
 * Mixing (Day 22) and recording (Day 23) consume the streams this hands out.
 */
export class SourceManager {
  private display: MediaStream | null = null;
  private mic: MediaStream | null = null;
  private camera: MediaStream | null = null;
  private readonly displayEndedSubject = new Subject<void>();

  /** Fires when the display stream ends outside our control, e.g. the browser's "Stop sharing". */
  readonly displayEnded$: Observable<void> = this.displayEndedSubject.asObservable();

  /** Current microphones; emits on subscribe and again whenever devices change. */
  readonly mics$: Observable<MicDevice[]> = new Observable<MicDevice[]>((subscriber) => {
    const emit = () => {
      this.listMics().then(
        (mics) => subscriber.next(mics),
        (error: unknown) => subscriber.error(toCaptureError(error)),
      );
    };
    emit();
    this.devices?.addEventListener('devicechange', emit);
    return () => this.devices?.removeEventListener('devicechange', emit);
  });

  constructor(
    private readonly devices: MediaDevicesPort | undefined = globalThis.navigator?.mediaDevices,
  ) {}

  get currentDisplay(): MediaStream | null {
    return this.display;
  }

  get currentMic(): MediaStream | null {
    return this.mic;
  }

  get currentCamera(): MediaStream | null {
    return this.camera;
  }

  /** Opens the webcam (video only; the mic is separate). Replaces any previous camera. */
  async openCamera(): Promise<MediaStream> {
    const devices = this.require('getUserMedia');
    let stream: MediaStream;
    try {
      stream = await devices.getUserMedia({ video: true, audio: false });
    } catch (error) {
      throw toCaptureError(error);
    }
    this.stopCamera();
    this.camera = stream;
    return stream;
  }

  stopCamera(): void {
    const stream = this.camera;
    this.camera = null;
    stream?.getTracks().forEach((track) => track.stop());
  }

  /** Opens the browser's screen/window/tab picker. Replaces (and stops) any previous display. */
  async pickDisplay(options: DisplayOptions): Promise<MediaStream> {
    const devices = this.require('getDisplayMedia');
    let stream: MediaStream;
    try {
      stream = await devices.getDisplayMedia({
        video: {
          frameRate: options.frameRate ?? DEFAULT_FRAME_RATE,
          ...(options.height ? { height: { ideal: options.height } } : {}),
          ...(options.cursor ? { cursor: options.cursor } : {}),
        },
        audio: options.systemAudio,
      });
    } catch (error) {
      throw toCaptureError(error);
    }
    this.stopDisplay();
    this.display = stream;
    const [video] = stream.getVideoTracks();
    if (video) {
      const unsubscribe = onTrackEnded(video, () => {
        unsubscribe();
        if (this.display === stream) {
          this.display = null;
          this.displayEndedSubject.next();
        }
      });
    }
    return stream;
  }

  /** Opens a microphone: the given device exactly, or the browser default. Replaces any previous. */
  async openMic(deviceId?: string, processing?: AudioProcessing): Promise<MediaStream> {
    const devices = this.require('getUserMedia');
    let stream: MediaStream;
    try {
      stream = await devices.getUserMedia({
        audio:
          deviceId || processing
            ? { ...(deviceId ? { deviceId: { exact: deviceId } } : {}), ...processing }
            : true,
        video: false,
      });
    } catch (error) {
      throw toCaptureError(error);
    }
    this.stopMic();
    this.mic = stream;
    return stream;
  }

  async listMics(): Promise<MicDevice[]> {
    const devices = this.require('enumerateDevices');
    const all = await devices.enumerateDevices();
    return all
      .filter((device) => device.kind === 'audioinput')
      .map((device, index) => ({
        deviceId: device.deviceId,
        label: device.label || `Microphone ${index + 1}`,
      }));
  }

  /**
   * Re-aims the shared screen's capture at a preset after it was picked. Best effort: the
   * browser decides what the surface can deliver, and a refusal leaves the old settings.
   */
  async applyQuality(preset: QualityPreset): Promise<void> {
    const [track] = this.display?.getVideoTracks() ?? [];
    try {
      await track?.applyConstraints({
        frameRate: preset.fps,
        height: { ideal: preset.height },
      });
    } catch {
      // Keep what the browser gave us.
    }
  }

  stopDisplay(): void {
    const stream = this.display;
    this.display = null;
    stream?.getTracks().forEach((track) => track.stop());
  }

  stopMic(): void {
    const stream = this.mic;
    this.mic = null;
    stream?.getTracks().forEach((track) => track.stop());
  }

  stopAll(): void {
    this.stopDisplay();
    this.stopMic();
    this.stopCamera();
  }

  private require(method: keyof MediaDevicesPort): MediaDevicesPort {
    if (!this.devices || typeof this.devices[method] !== 'function') {
      throw new CaptureError(
        'not-supported',
        `mediaDevices.${String(method)} is unavailable (unsupported browser or insecure context)`,
      );
    }
    return this.devices;
  }
}
