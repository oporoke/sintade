import type { Message, PingReply, WhoAmIReply } from '../shared/messages';

const $ = <T extends HTMLElement>(id: string) => document.querySelector<T>(`[data-testid="${id}"]`);

async function send<R>(message: Message): Promise<R> {
  return (await chrome.runtime.sendMessage(message)) as R;
}

/** Shows who is signed in (or how to sign in), as the web app's session says. */
export function render(reply: WhoAmIReply, root: ParentNode = document): void {
  const status = root.querySelector<HTMLElement>('[data-testid="popup-status"]');
  const signIn = root.querySelector<HTMLButtonElement>('[data-testid="popup-sign-in"]');
  const record = root.querySelector<HTMLButtonElement>('[data-testid="popup-record"]');
  if (!status || !signIn || !record) {
    return;
  }
  signIn.hidden = reply.state !== 'signed-out';
  record.disabled = true; // recording arrives with the tab-capture days
  switch (reply.state) {
    case 'signed-in':
      status.textContent = `Signed in as ${reply.email}`;
      status.dataset['state'] = 'signed-in';
      break;
    case 'signed-out':
      status.textContent = 'Sign in to Sintade to record';
      status.dataset['state'] = 'signed-out';
      break;
    case 'unreachable':
      status.textContent = "Can't reach Sintade. Check your connection.";
      status.dataset['state'] = 'unreachable';
      break;
  }
}

async function main(): Promise<void> {
  try {
    const ping = await send<PingReply>({ type: 'ping' });
    const version = $('popup-version');
    if (version) version.textContent = `Version ${ping.version}`;
    render(await send<WhoAmIReply>({ type: 'whoami' }));
  } catch {
    const status = $('popup-status');
    if (status) status.textContent = "Sintade couldn't start. Reload the extension.";
  }
  $('popup-sign-in')?.addEventListener('click', () => {
    void send({ type: 'open-sign-in' }).then(() => window.close());
  });
}

if (typeof chrome !== 'undefined' && chrome.runtime?.id) {
  void main();
}
