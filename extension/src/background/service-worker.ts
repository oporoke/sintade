import { SintadeApi } from './api';
import { handleMessage } from './handlers';
import { Recorder, RecorderPlatform } from './recorder';
import type { OffscreenMessage, StateReply } from '../shared/messages';
import { isRecordingStateEvent } from '../shared/messages';
import type { RecordingState } from '../shared/recording';
import { IDLE } from '../shared/recording';
import { SETTINGS_KEY, normalizeSettings } from '../shared/settings';

declare const __SINTADE_ORIGIN__: string;

const STATE_KEY = 'recording';
const TAB_KEY = 'recording-tab';
const OFFSCREEN_URL = 'offscreen.html';

const version = chrome.runtime.getManifest().version;

const platform: RecorderPlatform = {
  getStreamId: (tabId) => chrome.tabCapture.getMediaStreamId({ targetTabId: tabId }),
  async hasOffscreen() {
    const contexts = await chrome.runtime.getContexts({
      contextTypes: [chrome.runtime.ContextType.OFFSCREEN_DOCUMENT],
    });
    return contexts.length > 0;
  },
  async ensureOffscreen() {
    if (await this.hasOffscreen()) {
      return;
    }
    await chrome.offscreen.createDocument({
      url: OFFSCREEN_URL,
      reasons: [chrome.offscreen.Reason.USER_MEDIA],
      justification: 'Record the current tab with MediaRecorder and upload it while recording.',
    });
  },
  async closeOffscreen() {
    if (await this.hasOffscreen()) {
      await chrome.offscreen.closeDocument();
    }
  },
  async sendToOffscreen(message: OffscreenMessage) {
    return (await chrome.runtime.sendMessage(message)) as StateReply | undefined;
  },
  async startOverlay(tabId, settings) {
    await chrome.scripting.executeScript({ target: { tabId }, files: ['content.js'] });
    await chrome.tabs.sendMessage(tabId, { type: 'overlay-start', ...settings });
  },
  async stopOverlay(tabId) {
    await chrome.tabs.sendMessage(tabId, { type: 'overlay-stop' });
  },
  async loadSettings() {
    const stored = await chrome.storage.local.get(SETTINGS_KEY);
    return normalizeSettings(stored[SETTINGS_KEY]);
  },
  async loadTab() {
    const stored = await chrome.storage.session.get(TAB_KEY);
    return (stored[TAB_KEY] as number | null | undefined) ?? null;
  },
  async saveTab(tabId) {
    await chrome.storage.session.set({ [TAB_KEY]: tabId });
  },
  async loadState() {
    const stored = await chrome.storage.session.get(STATE_KEY);
    return (stored[STATE_KEY] as RecordingState | undefined) ?? IDLE;
  },
  async saveState(state) {
    await chrome.storage.session.set({ [STATE_KEY]: state });
  },
};

const recorder = new Recorder(platform, { origin: __SINTADE_ORIGIN__, version });
const env = {
  version,
  api: new SintadeApi(__SINTADE_ORIGIN__, version),
  recorder,
  openTab: (url: string) => chrome.tabs.create({ url }),
};

chrome.runtime.onMessage.addListener((message: unknown, _sender, sendResponse) => {
  if (isRecordingStateEvent(message)) {
    void recorder.onState(message.state);
    return false;
  }
  void handleMessage(message, env).then((reply) => {
    if (reply !== null) {
      sendResponse(reply);
    }
  });
  // Keep the channel open for the async reply (a message that isn't ours just gets no answer).
  return true;
});
