import { handleMessage } from './handlers';

const env = { version: chrome.runtime.getManifest().version };

chrome.runtime.onMessage.addListener((message: unknown, _sender, sendResponse) => {
  const reply = handleMessage(message, env);
  if (reply === null) {
    return false;
  }
  sendResponse(reply);
  return false;
});
