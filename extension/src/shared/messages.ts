/** Messages between the popup, the service worker and (later) the offscreen document. */
export interface PingMessage {
  type: 'ping';
}

export type Message = PingMessage;

export interface PingReply {
  ok: true;
  version: string;
}

export function isMessage(value: unknown): value is Message {
  return (
    typeof value === 'object' &&
    value !== null &&
    'type' in value &&
    (value as { type: unknown }).type === 'ping'
  );
}
