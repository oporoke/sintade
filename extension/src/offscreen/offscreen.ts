import { AudioMixer, TakeSession, Uploader, openChunkStore } from '@capture';

import { SintadeApi } from '../background/api';
import type { OffscreenMessage, RecordingStateEvent, StateReply } from '../shared/messages';
import { isOffscreenMessage } from '../shared/messages';
import type { RecordingState } from '../shared/recording';
import { IDLE } from '../shared/recording';
import { RecordingController } from './recording-controller';
import { ExtensionUploadApi } from './upload-api';

let controller: RecordingController | null = null;
let lastState: RecordingState = IDLE;
/** Keeps the tab's own sound audible: capturing a tab mutes it for the user. */
let playback: AudioContext | null = null;

function publish(state: RecordingState): void {
  lastState = state;
  const event: RecordingStateEvent = { type: 'recording-state', state };
  void chrome.runtime.sendMessage(event).catch(() => undefined);
  if (state.phase !== 'starting' && state.phase !== 'recording' && state.phase !== 'uploading') {
    void playback?.close();
    playback = null;
  }
}

/** The captured tab as a MediaStream (video, and audio if the tab plays any). */
async function tabStream(streamId: string): Promise<MediaStream> {
  const constraint = {
    mandatory: { chromeMediaSource: 'tab', chromeMediaSourceId: streamId },
  };
  return navigator.mediaDevices.getUserMedia({
    audio: constraint,
    video: {
      mandatory: {
        ...constraint.mandatory,
        maxWidth: 1920,
        maxHeight: 1080,
        maxFrameRate: 30,
      },
    },
  } as unknown as MediaStreamConstraints);
}

async function start(message: Extract<OffscreenMessage, { type: 'offscreen-start' }>) {
  if (controller && ['starting', 'recording', 'uploading'].includes(lastState.phase)) {
    return;
  }
  const api = new SintadeApi(message.origin, message.version);
  const uploadApi = new ExtensionUploadApi(api);
  let stream: MediaStream;
  try {
    stream = await tabStream(message.streamId);
  } catch (error) {
    publish({
      phase: 'error',
      code: 'capture-failed',
      message: error instanceof Error ? error.message : 'The tab could not be captured.',
    });
    return;
  }
  if (stream.getAudioTracks().length > 0) {
    playback = new AudioContext();
    playback.createMediaStreamSource(stream).connect(playback.destination);
  }
  const store = await openChunkStore();
  controller = new RecordingController({
    api: uploadApi,
    stream,
    store,
    mixer: new AudioMixer(),
    startTake: (options) => TakeSession.start(options),
    createUploader: (takeId) => new Uploader({ api: uploadApi, store, takeId }),
    linkUrl: (slug) => `${message.origin}/s/${slug}`,
    onState: publish,
  });
  await controller.start();
}

chrome.runtime.onMessage.addListener((message: unknown, _sender, sendResponse) => {
  if (!isOffscreenMessage(message)) {
    return false;
  }
  switch (message.type) {
    case 'offscreen-start':
      void start(message).then(() => sendResponse({ state: lastState } satisfies StateReply));
      return true;
    case 'offscreen-stop':
      void (controller?.stop() ?? Promise.resolve()).then(() =>
        sendResponse({ state: lastState } satisfies StateReply),
      );
      return true;
    case 'offscreen-status':
      sendResponse({ state: lastState } satisfies StateReply);
      return false;
  }
});
