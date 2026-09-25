import { HttpErrorResponse } from '@angular/common/http';

import { isProblemDetails } from './problem-details';

export function extractErrorMessage(error: unknown): string {
  if (error instanceof HttpErrorResponse && isProblemDetails(error.error)) {
    return error.error.detail;
  }
  return $localize`Something went wrong. Please try again.`;
}
