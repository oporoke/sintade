/** What the recording is doing, as the popup shows it. The offscreen document is the source of truth. */
export type RecordingState =
  | { phase: 'idle' }
  | { phase: 'starting' }
  | { phase: 'recording'; startedAt: number; maxDurationMs: number | null }
  | { phase: 'uploading' }
  | { phase: 'done'; recordingId: string; url: string }
  | { phase: 'error'; code: RecordingErrorCode; message: string };

export type RecordingErrorCode =
  | 'not-signed-in'
  | 'limit-reached'
  | 'unreachable'
  | 'capture-failed'
  | 'microphone'
  | 'upload-failed';

export const IDLE: RecordingState = { phase: 'idle' };

/** Is a recording in progress (so a second one must not start)? */
export function isBusy(state: RecordingState): boolean {
  return state.phase === 'starting' || state.phase === 'recording' || state.phase === 'uploading';
}
