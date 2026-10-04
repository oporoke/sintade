/** Messages between the popup, the service worker and (later) the offscreen document. */
export interface PingMessage {
  type: 'ping';
}

/** Who is signed in to Sintade in this browser? */
export interface WhoAmIMessage {
  type: 'whoami';
}

export interface OpenSignInMessage {
  type: 'open-sign-in';
}

export type Message = PingMessage | WhoAmIMessage | OpenSignInMessage;

export interface PingReply {
  ok: true;
  version: string;
}

export interface Workspace {
  id: string;
  name: string;
  role: string;
}

/** The answer to `whoami`. `unreachable`: the server didn't answer (offline, wrong address). */
export type WhoAmIReply =
  | { state: 'signed-in'; email: string; displayName: string; workspace: Workspace | null }
  | { state: 'signed-out' }
  | { state: 'unreachable' };

export interface OkReply {
  ok: true;
}

const TYPES = new Set(['ping', 'whoami', 'open-sign-in']);

export function isMessage(value: unknown): value is Message {
  return (
    typeof value === 'object' &&
    value !== null &&
    'type' in value &&
    typeof (value as { type: unknown }).type === 'string' &&
    TYPES.has((value as { type: string }).type)
  );
}
