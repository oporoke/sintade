# ADR-0006: HTTPS dev server with a same-origin `/api` proxy

## Status

Accepted — 2026-09-25

## Context

Day 17's Playwright e2e (`web/e2e/signup-to-login.spec.ts`) passed on Chromium and Firefox
but failed on WebKit in CI: after a successful `POST /auth/login` the browser landed back on
`/login` instead of `/home`.

The first fix attempt (commit `bef1bd2`) assumed a timing race between `Set-Cookie` and the
next request, and stopped `AuthService.login()` from chaining into `fetchMe()`. CI still
failed the same way on WebKit, so that diagnosis was wrong, and the change has been reverted.

The real cause is the `Secure` attribute on every auth cookie (`sintade_session`,
`sintade_refresh`, `sintade_csrf`, set in `bin/api/src/session.rs` and
`bin/api/src/routes/auth.rs`). The SPA was served on `http://localhost:4200` and called the
API directly at `http://localhost:8080`. Chromium and Firefox treat `http://localhost` as a
secure context and store `Secure` cookies set over it. Playwright's WebKit on Linux does not:
it drops them silently. So `/me` never had a session cookie, and the route guard redirected
to `/login`. Local Safari developers would hit the same thing.

## Options considered

1. **Serve the SPA over HTTPS in dev and proxy `/api` through it (same origin).** `ng serve`
   gets `ssl: true` (Angular generates a self-signed cert) and a `proxy.conf.json` forwarding
   `/api` to `http://localhost:8080`. The browser only ever talks to
   `https://localhost:4200`, so `Secure` cookies are accepted by every engine. No backend
   change, no new dependency.
2. **A `COOKIE_SECURE` config flag, set to false in dev/test.** Smaller, but it relaxes a
   security control in some environments, and prod/dev cookie behaviour would diverge.
3. **Skip WebKit for this spec.** Loses cross-browser coverage of the Day 17 Check.

## Decision

Option 1.

- `web/angular.json` `serve.options`: `ssl: true`, `proxyConfig: proxy.conf.json`.
- `ApiClient` uses the relative base path `/api/v1`.
- `PUBLIC_BASE_URL` (email links, CORS allow-origin, MinIO CORS) becomes
  `https://localhost:4200` in `.env.example`, `compose.yml` and CI.
- Playwright uses `baseURL: https://localhost:4200` with `ignoreHTTPSErrors: true` for the
  self-signed cert.
- Cookies stay `Secure; HttpOnly; SameSite=Lax` everywhere.

## Consequences

- Developers open `https://localhost:4200` and accept the self-signed certificate once.
- The API's CORS layer (`allow_credentials`) is no longer used by the dev SPA, since it is now
  same-origin. It is kept for a possible separate-API-host production topology, which is still
  `TODO: Verify`.
- The `csrfInterceptor` stays origin-agnostic for the same reason.
- Future upload days: the HTTPS SPA will PUT to presigned MinIO URLs on `http://localhost:9000`.
  Browsers treat `http://localhost` as potentially trustworthy for mixed content, but that
  needs checking on WebKit once uploads land (`TODO: Verify`). MinIO's CORS origin already
  follows `PUBLIC_BASE_URL`.
