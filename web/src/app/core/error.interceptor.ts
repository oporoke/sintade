import { HttpErrorResponse, HttpInterceptorFn } from '@angular/common/http';
import { catchError, throwError } from 'rxjs';

import { isProblemDetails } from './problem-details';

export const errorInterceptor: HttpInterceptorFn = (req, next) => {
  return next(req).pipe(
    catchError((error: unknown) => {
      if (error instanceof HttpErrorResponse) {
        const requestId = error.headers.get('x-request-id');
        const problem = isProblemDetails(error.error) ? error.error : undefined;
        console.error('[api]', req.method, req.url, error.status, problem?.detail ?? error.message, {
          requestId,
        });
      }
      return throwError(() => error);
    }),
  );
};
