import {
  isRecordingStateEvent,
  type Message,
  type PingReply,
  type StateReply,
  type WhoAmIReply,
} from '../shared/messages';
import { isBusy, type RecordingState } from '../shared/recording';

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

/** Shows who is signed in (or how to sign in), as the web app's session says. */
export function renderAccount(reply: WhoAmIReply, root: ParentNode = document): void {
  const status = $('popup-status', root);
  const signIn = $<HTMLButtonElement>('popup-sign-in', root);
  if (!status || !signIn) {
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

/** Shows the recording: the Record button, the Stop button with a timer, or the finished link. */
export function renderRecording(
  state: RecordingState,
  signedIn: boolean,
  now: number,
  root: ParentNode = document,
): void {
  const record = $<HTMLButtonElement>('popup-record', root);
  const stop = $<HTMLButtonElement>('popup-stop', root);
  const line = $('popup-recording', root);
  const result = $('popup-result', root);
  const link = $<HTMLAnchorElement>('popup-link', root);
  if (!record || !stop || !line || !result || !link) {
    return;
  }
  const busy = isBusy(state);
  record.hidden = busy;
  record.disabled = !signedIn;
  stop.hidden = state.phase !== 'recording';
  line.hidden = !busy && state.phase !== 'error';
  line.dataset['phase'] = state.phase;
  result.hidden = state.phase !== 'done';
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
    case 'done':
      link.href = state.url;
      link.textContent = state.url;
      break;
    case 'idle':
      line.textContent = '';
      break;
  }
}

async function activeTabId(): Promise<number | null> {
  const [tab] = await chrome.tabs.query({ active: true, currentWindow: true });
  return tab?.id ?? null;
}

async function main(): Promise<void> {
  let signedIn = false;
  let state: RecordingState = { phase: 'idle' };
  const draw = () => renderRecording(state, signedIn, Date.now());

  try {
    const ping = await send<PingReply>({ type: 'ping' });
    const version = $('popup-version');
    if (version) version.textContent = `Version ${ping.version}`;
    const account = await send<WhoAmIReply>({ type: 'whoami' });
    signedIn = account.state === 'signed-in';
    renderAccount(account);
    state = (await send<StateReply>({ type: 'recording-status' })).state;
    draw();
  } catch {
    const status = $('popup-status');
    if (status) status.textContent = "Sintade couldn't start. Reload the extension.";
    return;
  }

  $('popup-sign-in')?.addEventListener('click', () => {
    void send({ type: 'open-sign-in' }).then(() => window.close());
  });
  $('popup-record')?.addEventListener('click', () => {
    void (async () => {
      const tabId = await activeTabId();
      if (tabId === null) {
        return;
      }
      state = { phase: 'starting' };
      draw();
      state = (await send<StateReply>({ type: 'start-recording', tabId })).state;
      draw();
    })();
  });
  $('popup-stop')?.addEventListener('click', () => {
    void send<StateReply>({ type: 'stop-recording' }).then((reply) => {
      state = reply.state;
      draw();
    });
  });
  chrome.runtime.onMessage.addListener((message: unknown) => {
    if (isRecordingStateEvent(message)) {
      state = message.state;
      draw();
    }
  });
  setInterval(draw, 500); // the timer
}

if (typeof chrome !== 'undefined' && chrome.runtime?.id) {
  void main();
}
