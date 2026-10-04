import { Message, PingReply, isMessage } from '../shared/messages';

export interface Env {
  version: string;
}

/** What the service worker answers to a message from an extension page; `null` for anything else. */
export function handleMessage(message: unknown, env: Env): PingReply | null {
  if (!isMessage(message)) {
    return null;
  }
  const known: Message = message;
  switch (known.type) {
    case 'ping':
      return { ok: true, version: env.version };
  }
}
