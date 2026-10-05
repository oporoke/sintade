import { SintadeApi } from './api';
import { handleMessage } from './handlers';

declare const __SINTADE_ORIGIN__: string;

const version = chrome.runtime.getManifest().version;
const env = {
  version,
  api: new SintadeApi(__SINTADE_ORIGIN__, version),
  openTab: (url: string) => chrome.tabs.create({ url }),
};

chrome.runtime.onMessage.addListener((message: unknown, _sender, sendResponse) => {
  void handleMessage(message, env).then((reply) => {
    if (reply !== null) {
      sendResponse(reply);
    }
  });
  // Keep the channel open for the async reply (a message that isn't ours just gets no answer).
  return true;
});
