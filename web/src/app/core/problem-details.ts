/** RFC 9457 problem+json shape returned by the API on error (see bin/api/src/error.rs). */
export interface ProblemDetails {
  type: string;
  title: string;
  status: number;
  detail: string;
}

export function isProblemDetails(value: unknown): value is ProblemDetails {
  return (
    typeof value === 'object' &&
    value !== null &&
    'title' in value &&
    'status' in value &&
    'detail' in value
  );
}
