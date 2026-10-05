import type { RecordingState } from './recording';

/** Messages between the popup, the service worker and the offscreen document. */
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

/** Popup → service worker: record `tabId`. Must come from a click (it grants `activeTab`). */
export interface StartRecordingMessage {
  type: 'start-recording';
  tabId: number;
}

export interface StopRecordingMessage {
  type: 'stop-recording';
}

export interface RecordingStatusMessage {
  type: 'recording-status';
}

export type Message =
  | PingMessage
  | WhoAmIMessage
  | OpenSignInMessage
  | StartRecordingMessage
  | StopRecordingMessage
  | RecordingStatusMessage;

/** Service worker → offscreen document. `target` keeps other listeners from answering. */
export interface OffscreenStart {
  target: 'offscreen';
  type: 'offscreen-start';
  streamId: string;
  origin: string;
  version: string;
}

export interface OffscreenStop {
  target: 'offscreen';
  type: 'offscreen-stop';
}

export interface OffscreenStatus {
  target: 'offscreen';
  type: 'offscreen-status';
}

export type OffscreenMessage = OffscreenStart | OffscreenStop | OffscreenStatus;

/** Offscreen document → everyone: the recording changed. */
export interface RecordingStateEvent {
  type: 'recording-state';
  state: RecordingState;
}

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

export interface StateReply {
  state: RecordingState;
}

const POPUP_TYPES = new Set([
  'ping',
  'whoami',
  'open-sign-in',
  'start-recording',
  'stop-recording',
  'recording-status',
]);

export function isMessage(value: unknown): value is Message {
  if (typeof value !== 'object' || value === null) {
    return false;
  }
  const { type } = value as { type?: unknown };
  if (typeof type !== 'string' || !POPUP_TYPES.has(type)) {
    return false;
  }
  return type !== 'start-recording' || Number.isInteger((value as { tabId?: unknown }).tabId);
}

export function isOffscreenMessage(value: unknown): value is OffscreenMessage {
  return (
    typeof value === 'object' &&
    value !== null &&
    (value as { target?: unknown }).target === 'offscreen' &&
    ['offscreen-start', 'offscreen-stop', 'offscreen-status'].includes(
      (value as { type?: string }).type ?? '',
    )
  );
}

export function isRecordingStateEvent(value: unknown): value is RecordingStateEvent {
  return (
    typeof value === 'object' &&
    value !== null &&
    (value as { type?: unknown }).type === 'recording-state' &&
    typeof (value as { state?: unknown }).state === 'object'
  );
}
