import { SETTINGS_KEY, normalizeSettings } from '../shared/settings';

/**
 * The microphone permission is asked for here, on a page of its own, because the toolbar popup
 * closes as soon as the browser's prompt appears and an offscreen document can't show one. Once
 * allowed, the offscreen document can use the microphone without asking.
 */
const status = document.querySelector<HTMLElement>('[data-testid="mic-status"]');
const retry = document.querySelector<HTMLButtonElement>('[data-testid="mic-retry"]');

async function ask(): Promise<void> {
  if (retry) retry.hidden = true;
  try {
    const stream = await navigator.mediaDevices.getUserMedia({ audio: true });
    stream.getTracks().forEach((track) => track.stop());
    const stored = await chrome.storage.local.get(SETTINGS_KEY);
    await chrome.storage.local.set({
      [SETTINGS_KEY]: { ...normalizeSettings(stored[SETTINGS_KEY]), microphone: true },
    });
    if (status) status.textContent = 'Done. The microphone is on. You can close this tab.';
    status?.setAttribute('data-state', 'granted');
    setTimeout(() => window.close(), 800);
  } catch {
    if (status) {
      status.textContent =
        'The microphone was blocked. Allow it in the address bar or the browser settings, then try again.';
    }
    status?.setAttribute('data-state', 'denied');
    if (retry) retry.hidden = false;
  }
}

retry?.addEventListener('click', () => void ask());
void ask();
