import { Problem } from './auth.models';

/** RFC 9457 problem+json shape returned by the API on error (generated from bin/api/src/error.rs). */
export type ProblemDetails = Problem;

export function isProblemDetails(value: unknown): value is ProblemDetails {
  return (
    typeof value === 'object' &&
    value !== null &&
    'title' in value &&
    'status' in value &&
    'detail' in value
  );
}
