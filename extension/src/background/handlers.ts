import type { OkReply, PingReply, WhoAmIReply } from '../shared/messages';
import { isMessage } from '../shared/messages';
import type { SintadeApi } from './api';

export interface Env {
  version: string;
  api: Pick<SintadeApi, 'whoAmI' | 'signInUrl'>;
  openTab: (url: string) => Promise<unknown>;
}

export type Reply = PingReply | WhoAmIReply | OkReply;

/** What the service worker answers to a message from an extension page; `null` for anything else. */
export async function handleMessage(message: unknown, env: Env): Promise<Reply | null> {
  if (!isMessage(message)) {
    return null;
  }
  switch (message.type) {
    case 'ping':
      return { ok: true, version: env.version };
    case 'whoami':
      return env.api.whoAmI();
    case 'open-sign-in':
      await env.openTab(env.api.signInUrl());
      return { ok: true };
  }
}
