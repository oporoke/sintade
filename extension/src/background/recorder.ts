import type { OffscreenMessage, StateReply } from '../shared/messages';
import type { RecordingState } from '../shared/recording';
import { IDLE, isBusy } from '../shared/recording';
import type { Settings } from '../shared/settings';

/** The browser APIs the recorder needs, so the logic is tested without a browser. */
export interface RecorderPlatform {
  /** `chrome.tabCapture.getMediaStreamId`: needs the extension to have been invoked on the tab. */
  getStreamId(tabId: number): Promise<string>;
  ensureOffscreen(): Promise<void>;
  closeOffscreen(): Promise<void>;
  hasOffscreen(): Promise<boolean>;
  sendToOffscreen(message: OffscreenMessage): Promise<StateReply | undefined>;
  /** Puts the overlay script into the tab (activeTab + scripting) and starts it. */
  startOverlay(tabId: number, settings: Settings): Promise<void>;
  stopOverlay(tabId: number): Promise<void>;
  loadSettings(): Promise<Settings>;
  loadTab(): Promise<number | null>;
  saveTab(tabId: number | null): Promise<void>;
  loadState(): Promise<RecordingState>;
  saveState(state: RecordingState): Promise<void>;
}

export interface RecorderEnv {
  origin: string;
  version: string;
}

/**
 * Starts and stops tab recordings. The recording itself runs in the offscreen document (a
 * service worker can't hold a media stream or run MediaRecorder); this only sets it up, relays
 * Stop, and remembers the last state so the popup can show it after it was closed.
 */
export class Recorder {
  constructor(
    private readonly platform: RecorderPlatform,
    private readonly env: RecorderEnv,
  ) {}

  async status(): Promise<RecordingState> {
    if (await this.platform.hasOffscreen()) {
      const reply = await this.platform.sendToOffscreen({
        target: 'offscreen',
        type: 'offscreen-status',
      });
      if (reply) {
        return reply.state;
      }
    }
    return this.platform.loadState();
  }

  async start(tabId: number): Promise<RecordingState> {
    const current = await this.status();
    if (isBusy(current)) {
      return current;
    }
    await this.record({ phase: 'starting' });
    await this.platform.saveTab(tabId);
    try {
      const streamId = await this.platform.getStreamId(tabId);
      await this.platform.ensureOffscreen();
      const reply = await this.platform.sendToOffscreen({
        target: 'offscreen',
        type: 'offscreen-start',
        streamId,
        origin: this.env.origin,
        version: this.env.version,
      });
      return reply?.state ?? (await this.platform.loadState());
    } catch (error) {
      const state: RecordingState = {
        phase: 'error',
        code: 'capture-failed',
        message:
          error instanceof Error && error.message
            ? `This tab can't be recorded: ${error.message}`
            : "This tab can't be recorded.",
      };
      await this.record(state);
      await this.platform.saveTab(null);
      await this.platform.closeOffscreen().catch(() => undefined);
      return state;
    }
  }

  async stop(): Promise<RecordingState> {
    const reply = await this.platform.sendToOffscreen({
      target: 'offscreen',
      type: 'offscreen-stop',
    });
    return reply?.state ?? this.platform.loadState();
  }

  /** The offscreen document reports every change; the last one is kept, and a finished one frees it. */
  async onState(state: RecordingState): Promise<void> {
    await this.record(state);
    const tabId = await this.platform.loadTab();
    if (state.phase === 'recording' && tabId !== null) {
      // Click highlights and keys appear in the recording because the tab's own picture is what
      // is captured. A tab that can't take the script (a browser page) is recorded without.
      await this.platform
        .startOverlay(tabId, await this.platform.loadSettings())
        .catch(() => undefined);
    } else if (state.phase !== 'starting' && state.phase !== 'recording' && tabId !== null) {
      await this.platform.stopOverlay(tabId).catch(() => undefined);
      if (state.phase !== 'uploading') {
        await this.platform.saveTab(null);
      }
    }
    if (state.phase === 'done' || state.phase === 'error') {
      await this.platform.closeOffscreen().catch(() => undefined);
    }
  }

  /** Back to idle (the popup dismissed a finished recording). */
  async dismiss(): Promise<void> {
    const state = await this.platform.loadState();
    if (!isBusy(state)) {
      await this.record(IDLE);
    }
  }

  private record(state: RecordingState): Promise<void> {
    return this.platform.saveState(state);
  }
}
