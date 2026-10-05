# ADR-0022: Hardening baseline: rate limits on every route, security headers, scans in CI

## Status

Accepted — 2026-10-05 (Day 67, M8).

## Context

M8 prepares for a public launch. Rate limits existed only on login and signup; headers had been
set on the API since Day 16 but never audited; ZAP, secret scanning and dependency audits had not
been run, and the audit job was informational (it had been failing since Day 41).

## Decision

1. **Every route is rate limited** by one middleware (`bin/api/src/rate_limit.rs`), on top of the
   login (5/min/IP+email) and signup (3/h/IP) limits. Fixed one-minute windows in Postgres:

   | Class | Paths | Limit | Counted per |
   | --- | --- | --- | --- |
   | `watch` | `/api/v1/s/*` | 120/min | address (§11) |
   | `ingest` | `/api/v1/takes/*` | 600/min | user (§11 "presign 600/min/user") |
   | `auth` | `/api/v1/auth/*` | 60/min | address |
   | `api` | everything else, including paths that match no route | 600/min | user, else address |

   Only `/healthz` and `/readyz` are exempt. `429` is problem+json with `Retry-After`. A test fails
   if a mounted route is not classified. If the counter store fails the request is let through
   (and logged): the limiter must not be able to take the API down.
2. **The address is the *rightmost* `X-Forwarded-For` entry.** The proxy appends what it saw; all
   to the left is client-controlled, and keying on it let an attacker mint a fresh bucket per
   request (the Day 12–16 code used the leftmost). The web container's nginx appends the client
   address; behind a CDN the real address must be restored first (`TODO: Verify` at deploy).
3. **`RATE_LIMIT_SCALE`** (default 1) multiplies the middleware's limits for the dev server and
   the e2e suite, which send everything from one address. It is not a production setting and
   never touches login/signup limits.
4. **Headers.** API responses add `frame-ancestors 'none'`, `form-action 'self'`, `X-Frame-Options:
   DENY`, `Cross-Origin-Opener-Policy`, `Cross-Origin-Resource-Policy: same-origin`, a fuller
   `Permissions-Policy`, and `Cache-Control: no-store` unless the handler set one. The SPA is
   served by nginx (`deploy/nginx.conf.template`) with the same set and a CSP of `script-src
   'self'` (Angular's inline critical-CSS script is switched off in production builds) and
   `style-src 'self' 'unsafe-inline'` (component styles). It also gets the SPA fallback it lacked
   (deep links such as `/s/<slug>` returned 404 from the old image) and proxies `/api/`.
5. **Accepted ZAP warnings,** listed in `scripts/zap-baseline.conf`: cacheable hashed assets,
   `style-src 'unsafe-inline'`, "modern web application" (informational), and COEP (it would
   require CORP headers on every object in the storage host).
6. **Scans.** `just zap` (baseline of the production web image plus an API scan from the OpenAPI
   contract, with an active pass), `just secrets` (gitleaks over the full history, config in
   `.gitleaks.toml`), `just audit`. CI's `audit` job is now blocking (`cargo audit --deny
   warnings`, `npm audit --audit-level=high` for `web` and `extension`) and a new `secrets` job
   runs gitleaks. The one real finding, a CI-only `SESSION_SECRET` committed in `ci.yml`, is
   replaced by a per-run random value; the old value stays allowlisted only because it is in
   history.
7. **Dependency upgrades** that clear the audit: Angular 22.1 → 22.2.1 (router SSR DoS advisory;
   the build tool chain's `piscina`), `yoke-derive` 0.8.4 (yanked 0.8.3).

## Consequences

- Scanning a route-less path or hammering one endpoint costs the attacker their bucket, not the
  server.
- One Postgres upsert per request. At the MVP's scale this is cheap next to the request itself;
  if it shows up in the p95, move the counters in front of Postgres (`tower-governor`, §6)
  without changing the policy table.
- ZAP's active scan runs against a local database only; it creates junk rows.
