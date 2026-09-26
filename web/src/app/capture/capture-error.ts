/** Why acquiring a capture source failed, normalised across browsers. */
export type CaptureErrorKind =
  /** The user (or a policy) refused, or closed the picker. Browsers don't distinguish these. */
  | 'permission-denied'
  /** No device matches, e.g. no microphone, or the chosen one was unplugged. */
  | 'no-device'
  /** The device exists but another application holds it. */
  | 'device-busy'
  /** This browser or context can't capture at all (no API, insecure origin). */
  | 'not-supported'
  | 'aborted'
  | 'unknown';

export class CaptureError extends Error {
  constructor(
    readonly kind: CaptureErrorKind,
    message: string,
    options?: { cause?: unknown },
  ) {
    super(message, options);
    this.name = 'CaptureError';
  }
}

/** Maps the `DOMException` names that `getUserMedia`/`getDisplayMedia` reject with. */
export function toCaptureError(error: unknown): CaptureError {
  if (error instanceof CaptureError) {
    return error;
  }
  const name = error instanceof Error ? error.name : '';
  const message = error instanceof Error ? error.message : String(error);
  switch (name) {
    case 'NotAllowedError':
    case 'SecurityError':
    case 'PermissionDeniedError':
      return new CaptureError('permission-denied', message, { cause: error });
    case 'NotFoundError':
    case 'OverconstrainedError':
    case 'DevicesNotFoundError':
      return new CaptureError('no-device', message, { cause: error });
    case 'NotReadableError':
    case 'TrackStartError':
      return new CaptureError('device-busy', message, { cause: error });
    case 'NotSupportedError':
    case 'TypeError':
      return new CaptureError('not-supported', message, { cause: error });
    case 'AbortError':
      return new CaptureError('aborted', message, { cause: error });
    default:
      return new CaptureError('unknown', message, { cause: error });
  }
}
