import type { OkReply, PingReply, StateReply, WhoAmIReply } from '../shared/messages';
import { isMessage } from '../shared/messages';
import type { SintadeApi } from './api';
import type { Recorder } from './recorder';

export interface Env {
  version: string;
  api: Pick<SintadeApi, 'whoAmI' | 'signInUrl'>;
  recorder: Pick<Recorder, 'start' | 'stop' | 'status'>;
  openTab: (url: string) => Promise<unknown>;
}

export type Reply = PingReply | WhoAmIReply | OkReply | StateReply;

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
    case 'start-recording':
      return { state: await env.recorder.start(message.tabId) };
    case 'stop-recording':
      return { state: await env.recorder.stop() };
    case 'recording-status':
      return { state: await env.recorder.status() };
  }
}
