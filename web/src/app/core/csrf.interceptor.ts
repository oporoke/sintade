import { HttpInterceptorFn } from '@angular/common/http';

const CSRF_COOKIE_NAME = 'sintade_csrf';
const CSRF_HEADER_NAME = 'X-CSRF-Token';

/**
 * Double-submit CSRF (see bin/api/src/csrf.rs): reads the `sintade_csrf` cookie and echoes it
 * back as a header on state-changing requests. Angular's built-in XSRF interceptor skips
 * cross-origin requests (`xsrfInterceptorFn` in @angular/common/http); dev is same-origin via
 * the `ng serve` proxy (ADR-0006), but production topology isn't decided yet, so this custom
 * interceptor stays origin-agnostic rather than relying on the built-in one.
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
