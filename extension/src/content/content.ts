import { createOverlay } from './overlay';

/** Injected into the recorded tab by the service worker (activeTab + scripting). */

interface OverlayMessage {
  type: 'overlay-start' | 'overlay-stop';
  highlights?: boolean;
  keystrokes?: boolean;
}

const KEY = '__sintadeOverlay__';
interface Holder {
  [KEY]?: () => void;
  __sintadeListener__?: (message: unknown) => void;
}
const holder = window as unknown as Holder;

function stop(): void {
  holder[KEY]?.();
  delete holder[KEY];
}

// Injected again (a re-injection after navigation, or two starts): replace, never stack. The
// isolated world survives re-injection, so the previous listener is still attached.
stop();
if (holder.__sintadeListener__) {
  chrome.runtime.onMessage.removeListener(holder.__sintadeListener__);
}
const listener = (message: unknown) => {
  const m = message as Partial<OverlayMessage> | null;
  if (m?.type === 'overlay-start') {
    stop();
    holder[KEY] = createOverlay(document, {
      highlights: m.highlights === true,
      keystrokes: m.keystrokes === true,
    });
  } else if (m?.type === 'overlay-stop') {
    stop();
  }
};
holder.__sintadeListener__ = listener;
chrome.runtime.onMessage.addListener(listener);
