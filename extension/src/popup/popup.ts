import {
  isRecordingStateEvent,
  type Message,
  type PingReply,
  type StateReply,
  type WhoAmIReply,
} from '../shared/messages';
import { isBusy, type RecordingErrorCode, type RecordingState } from '../shared/recording';
import {
  DEFAULT_SETTINGS,
  SETTINGS_KEY,
  normalizeSettings,
  type Settings,
} from '../shared/settings';
import { unrecordableReason } from '../shared/tabs';

declare const __SINTADE_ORIGIN__: string;

const $ = <T extends HTMLElement>(id: string, root: ParentNode = document) =>
  root.querySelector<T>(`[data-testid="${id}"]`);

async function send<R>(message: Message): Promise<R> {
  return (await chrome.runtime.sendMessage(message)) as R;
}

/** "0:05", "12:03": the recording timer. */
export function clock(ms: number): string {
  const total = Math.max(0, Math.floor(ms / 1000));
  return `${Math.floor(total / 60)}:${String(total % 60).padStart(2, '0')}`;
}

/** Which buttons a failed recording offers, by what went wrong. */
export function errorActions(code: RecordingErrorCode): {
  signIn: boolean;
  library: boolean;
  retry: boolean;
} {
  switch (code) {
    case 'not-signed-in':
      return { signIn: true, library: false, retry: false };
    case 'limit-reached':
      return { signIn: false, library: true, retry: false };
    case 'upload-failed':
      return { signIn: false, library: true, retry: true };
    default:
      return { signIn: false, library: false, retry: true };
  }
}

interface View {
  account: WhoAmIReply | null;
  state: RecordingState;
  tab: { id: number; title: string; url: string | undefined } | null;
  copied: boolean;
  micHint: boolean;
}

/** Shows who is signed in (or how to sign in), as the web app's session says. */
export function renderAccount(reply: WhoAmIReply | null, root: ParentNode = document): void {
  const status = $('popup-status', root);
  const signIn = $<HTMLButtonElement>('popup-sign-in', root);
  if (!status || !signIn || !reply) {
    return;
  }
  signIn.hidden = reply.state !== 'signed-out';
  status.dataset['state'] = reply.state;
  switch (reply.state) {
    case 'signed-in':
      status.textContent = `Signed in as ${reply.email}`;
      break;
    case 'signed-out':
      status.textContent = 'Sign in to Sintade to record';
      break;
    case 'unreachable':
      status.textContent = "Can't reach Sintade. Check your connection.";
      break;
  }
}

/** Draws everything below the account line from the view. */
export function render(view: View, now: number, root: ParentNode = document): void {
  const { state } = view;
  const signedIn = view.account?.state === 'signed-in';
  const busy = isBusy(state);
  const finished = state.phase === 'done' || state.phase === 'error';
  const blocked = view.tab ? unrecordableReason(view.tab.url) : null;

  const tab = $('popup-tab', root);
  const blockedLine = $('popup-blocked', root);
  const record = $<HTMLButtonElement>('popup-record', root);
  const stop = $<HTMLButtonElement>('popup-stop', root);
  const line = $('popup-recording', root);
  const options = $<HTMLFieldSetElement>('popup-options', root);
  const error = $('popup-error', root);
  const result = $('popup-result', root);
  const link = $<HTMLAnchorElement>('popup-link', root);
  const copy = $<HTMLButtonElement>('popup-copy', root);
  const copied = $('popup-copied', root);
  const hint = $('popup-mic-hint', root);
  if (!tab || !blockedLine || !record || !stop || !line || !options || !error || !result) {
    return;
  }

  tab.textContent = view.tab && !finished && !busy ? `Tab: ${view.tab.title || view.tab.url}` : '';
  blockedLine.hidden = blocked === null || busy || finished;
  blockedLine.textContent = blocked ?? '';
  options.hidden = busy || finished;
  if (hint) hint.hidden = !view.micHint;

  record.hidden = busy || finished;
  record.disabled = !signedIn || blocked !== null;
  stop.hidden = state.phase !== 'recording';

  line.hidden = !busy && state.phase !== 'error';
  line.dataset['phase'] = state.phase;
  switch (state.phase) {
    case 'starting':
      line.textContent = 'Starting…';
      break;
    case 'recording':
      line.textContent = `Recording ${clock(now - state.startedAt)}`;
      break;
    case 'uploading':
      line.textContent = 'Finishing the upload…';
      break;
    case 'error':
      line.textContent = state.message;
      break;
    default:
      line.textContent = '';
  }

  const actions = state.phase === 'error' ? errorActions(state.code) : null;
  error.hidden = actions === null;
  const set = (id: string, visible: boolean) => {
    const button = $(id, root);
    if (button) button.hidden = !visible;
  };
  set('popup-error-sign-in', actions?.signIn ?? false);
  set('popup-error-library', actions?.library ?? false);
  set('popup-error-retry', actions?.retry ?? false);

  result.hidden = state.phase !== 'done';
  if (state.phase === 'done' && link) {
    link.href = state.url;
    link.textContent = state.url;
  }
  if (copy) copy.hidden = view.copied;
  if (copied) copied.hidden = !view.copied;
}

async function activeTab(): Promise<View['tab']> {
  // `?tabId=` is how automated tests aim the popup at a tab; the toolbar never sets it.
  const forced = Number(new URLSearchParams(location.search).get('tabId'));
  const [tab] =
    Number.isInteger(forced) && forced > 0
      ? [await chrome.tabs.get(forced).catch(() => undefined)]
      : await chrome.tabs.query({ active: true, currentWindow: true });
  return tab?.id === undefined ? null : { id: tab.id, title: tab.title ?? '', url: tab.url };
}

async function writeClipboard(text: string): Promise<boolean> {
  try {
    await navigator.clipboard.writeText(text);
    return true;
  } catch {
    return false;
  }
}

/**
 * Asks the clipboard to take the link *now*, while the click that asked for it still counts as a
 * gesture, with the link itself to come: a ClipboardItem may hold a promise. `resolve` is called
 * with the link when the recording is done (or `reject` if it fails).
 */
function armClipboard(): { resolve(url: string): void; reject(): void; copied: Promise<boolean> } {
  let resolve!: (blob: Blob) => void;
  let reject!: (reason?: unknown) => void;
  const pending = new Promise<Blob>((res, rej) => {
    resolve = res;
    reject = rej;
  });
  pending.catch(() => undefined);
  let copied: Promise<boolean>;
  try {
    copied = navigator.clipboard
      .write([new ClipboardItem({ 'text/plain': pending })])
      .then(() => true)
      .catch(() => false);
  } catch {
    copied = Promise.resolve(false);
  }
  return {
    resolve: (url) => resolve(new Blob([url], { type: 'text/plain' })),
    reject: () => reject(new Error('no link')),
    copied,
  };
}

async function loadSettings(): Promise<Settings> {
  const stored = await chrome.storage.local.get(SETTINGS_KEY);
  return normalizeSettings(stored[SETTINGS_KEY] ?? DEFAULT_SETTINGS);
}

function bindOptions(settings: Settings, onMicHint: (shown: boolean) => void): void {
  for (const input of document.querySelectorAll<HTMLInputElement>('[data-setting]')) {
    const key = input.dataset['setting'] as keyof Settings;
    input.checked = settings[key];
    input.addEventListener('change', () => {
      void (async () => {
        if (key === 'microphone' && input.checked) {
          // The browser's prompt can't be shown from here: send the user to a page that can.
          const permission = await navigator.permissions
            .query({ name: 'microphone' as PermissionName })
            .catch(() => null);
          if (permission?.state !== 'granted') {
            input.checked = false;
            onMicHint(true);
            await chrome.tabs.create({ url: chrome.runtime.getURL('mic.html') });
            return;
          }
        }
        onMicHint(false);
        const current = await loadSettings();
        await chrome.storage.local.set({ [SETTINGS_KEY]: { ...current, [key]: input.checked } });
      })();
    });
  }
}

async function main(): Promise<void> {
  const view: View = {
    account: null,
    state: { phase: 'idle' },
    tab: null,
    copied: false,
    micHint: false,
  };
  const draw = () => render(view, Date.now());
  const wasBusy = () => isBusy(view.state);

  try {
    const ping = await send<PingReply>({ type: 'ping' });
    const version = $('popup-version');
    if (version) version.textContent = `Version ${ping.version}`;
    view.account = await send<WhoAmIReply>({ type: 'whoami' });
    renderAccount(view.account);
    view.state = (await send<StateReply>({ type: 'recording-status' })).state;
    view.tab = await activeTab();
    bindOptions(await loadSettings(), (shown) => {
      view.micHint = shown;
      draw();
    });
    draw();
  } catch {
    const status = $('popup-status');
    if (status) status.textContent = "Sintade couldn't start. Reload the extension.";
    return;
  }

  let armed: ReturnType<typeof armClipboard> | null = null;
  const apply = (state: RecordingState) => {
    const justFinished = wasBusy() && state.phase === 'done';
    view.state = state;
    if (state.phase !== 'done') view.copied = false;
    draw();
    if (state.phase === 'done' || state.phase === 'error') {
      const pending = armed;
      armed = null;
      if (state.phase === 'error') {
        pending?.reject();
      } else if (pending) {
        // Stop was clicked here: the clipboard is already waiting for the link.
        pending.resolve(state.url);
        void pending.copied.then((ok) => {
          view.copied = ok;
          draw();
        });
      } else if (justFinished) {
        // The popup was opened after Stop, or the limit stopped it: try, else offer the button.
        void writeClipboard(state.url).then((ok) => {
          view.copied = ok;
          draw();
        });
      }
    }
  };

  $('popup-sign-in')?.addEventListener('click', () => {
    void send({ type: 'open-sign-in' }).then(() => window.close());
  });
  $('popup-error-sign-in')?.addEventListener('click', () => {
    void send({ type: 'open-sign-in' }).then(() => window.close());
  });
  $('popup-error-library')?.addEventListener('click', () => {
    void chrome.tabs.create({ url: `${__SINTADE_ORIGIN__}/library` }).then(() => window.close());
  });
  $('popup-record')?.addEventListener('click', () => {
    void (async () => {
      const tab = await activeTab();
      if (tab === null) return;
      view.tab = tab;
      apply({ phase: 'starting' });
      apply((await send<StateReply>({ type: 'start-recording', tabId: tab.id })).state);
    })();
  });
  $('popup-stop')?.addEventListener('click', () => {
    armed ??= armClipboard();
    void send<StateReply>({ type: 'stop-recording' }).then((reply) => apply(reply.state));
  });
  const reset = () =>
    void send<StateReply>({ type: 'dismiss-recording' }).then((reply) => apply(reply.state));
  $('popup-error-retry')?.addEventListener('click', reset);
  $('popup-another')?.addEventListener('click', reset);
  $('popup-copy')?.addEventListener('click', () => {
    if (view.state.phase !== 'done') return;
    void writeClipboard(view.state.url).then((ok) => {
      view.copied = ok;
      draw();
    });
  });
  chrome.runtime.onMessage.addListener((message: unknown) => {
    if (isRecordingStateEvent(message)) apply(message.state);
  });
  setInterval(draw, 500); // the timer
}

if (typeof chrome !== 'undefined' && chrome.runtime?.id) {
  void main();
}
