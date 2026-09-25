import { HttpErrorResponse, HttpInterceptorFn } from '@angular/common/http';
import { inject } from '@angular/core';
import { catchError, switchMap, throwError } from 'rxjs';

import { AuthService } from './auth.service';

/** Requests to these already handle their own 401s (wrong credentials, expired/invalid token,
 * no session yet) as a normal outcome, not as "the access cookie just expired" -- retrying them
 * via a silent refresh would be meaningless or actively wrong. */
const SILENT_REFRESH_EXEMPT_PATHS = [
  '/auth/login',
  '/auth/register',
  '/auth/refresh',
  '/auth/logout',
  '/auth/verify-email',
  '/auth/password/forgot',
  '/auth/password/reset',
];

/**
 * Silent refresh (Day 17): a 401 on any other request most likely means the 15-minute access
 * cookie just expired while the 30-day refresh cookie is still good. Exchange it transparently
 * and retry the original request once, so the user isn't bounced to /login for that reason.
 */
export const silentRefreshInterceptor: HttpInterceptorFn = (req, next) => {
  if (SILENT_REFRESH_EXEMPT_PATHS.some((path) => req.url.includes(path))) {
    return next(req);
  }

  const authService = inject(AuthService);

  return next(req).pipe(
    catchError((error: unknown) => {
      if (error instanceof HttpErrorResponse && error.status === 401) {
        return authService.refresh().pipe(switchMap(() => next(req)));
      }
      return throwError(() => error);
    }),
  );
};
