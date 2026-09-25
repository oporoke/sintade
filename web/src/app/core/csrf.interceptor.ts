import { HttpInterceptorFn } from '@angular/common/http';

const CSRF_COOKIE_NAME = 'sintade_csrf';
const CSRF_HEADER_NAME = 'X-CSRF-Token';

/**
 * Double-submit CSRF (see bin/api/src/csrf.rs): reads the `sintade_csrf` cookie and echoes it
 * back as a header on state-changing requests. Angular ships a built-in XSRF interceptor, but
 * it deliberately skips cross-origin requests (`xsrfInterceptorFn` in
 * @angular/common/http) — the SPA (:4200) and API (:8080) are on different origins by design
 * here, so that built-in interceptor is a no-op for this app and a custom one is needed instead.
 */
export const csrfInterceptor: HttpInterceptorFn = (req, next) => {
  if (req.method === 'GET' || req.method === 'HEAD') {
    return next(req);
  }
  const token = readCookie(CSRF_COOKIE_NAME);
  if (token) {
    req = req.clone({ headers: req.headers.set(CSRF_HEADER_NAME, token) });
  }
  return next(req);
};

function readCookie(name: string): string | null {
  const match = document.cookie.match(new RegExp(`(?:^|; )${name}=([^;]*)`));
  return match ? decodeURIComponent(match[1]) : null;
}
