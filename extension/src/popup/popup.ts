import type { Message, PingReply } from '../shared/messages';

const status = document.querySelector<HTMLElement>('[data-testid="popup-status"]');
const version = document.querySelector<HTMLElement>('[data-testid="popup-version"]');

async function ping(): Promise<PingReply> {
  const message: Message = { type: 'ping' };
  return (await chrome.runtime.sendMessage(message)) as PingReply;
}

async function main(): Promise<void> {
  try {
    const reply = await ping();
    if (status) status.textContent = 'Ready';
    if (version) version.textContent = `Version ${reply.version}`;
  } catch {
    if (status) status.textContent = "Sintade couldn't start. Reload the extension.";
  }
}

void main();
