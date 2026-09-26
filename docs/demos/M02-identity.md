# M2 — Identity demo (Days 11–20)

Date: 2026-09-26
Commit: `main` @ `0f7c171` (Day 19 + the rustls-webpki audit fix), demo branch `feat/day-020-m2-demo`
Environment: **local** (Docker Compose Postgres/MinIO/Mailpit, `api` + `worker` binaries, `ng serve` over HTTPS).
The planned demo was on staging, but no staging VPS exists yet. By the user's decision on
2026-09-26 it ran locally, and staging carries over (see below).

## Scope

`docs/design.md` §12, weeks 3–4: "Register, login, refresh rotation, logout, email
verification, password reset; personal workspace." The plan also put the Angular auth screens
(Day 17), tenancy/`authorize()` and the tenant-isolation harness (Day 18), and profile
settings, logout everywhere and the OpenAPI contract (Day 19) in this milestone.

The demo's headline, from the plan: **a new user signs up, verifies, and logs in**.

## How to run it yourself

```bash
just deps-up && just db-migrate
just api                      # terminal 1 -> :8080
just worker                   # terminal 2 (delivers email via Mailpit)
just web                      # terminal 3 -> https://localhost:4200 (accept the self-signed cert once)
```

### A. Through the UI (the headline)

1. Open `https://localhost:4200/signup`, enter an email, a display name and a password of 10+
   characters, then submit. **Expect:** a confirmation telling you a verification email was sent.
2. Open Mailpit at `http://localhost:8025` and click the "Verify your Sintade account" link.
   **Expect:** the verify page confirms.
3. Go to `/login` and log in. **Expect:** `/home` shows your email.
4. Click **Profile settings** and change your display name. **Expect:** "Saved.", and the name
   is still there after a reload.
5. Click **Log out everywhere**. **Expect:** you land on `/login`, and `/home` redirects back
   to `/login`.

Automated: `npx playwright test` (`e2e/signup-to-login.spec.ts`, `e2e/profile-settings.spec.ts`).

### B. Against the API (every acceptance criterion)

A scripted walkthrough with `curl` + Mailpit's REST API + `psql`. The results below were
captured on 2026-09-26 against the real running services. Nothing was mocked.

## Results

### US-01 — sign up

| Step | Result |
| --- | --- |
| `POST /auth/register` | `201` "if the details are valid, a verification email has been sent" |
| Same email again | `201`, identical body (no enumeration) |
| `password1234` (breached list) | `422` "password appears in a known data breach; choose a different one" |
| 9-character password | `422` "password must be at least 10 characters" |
| DB after signup | `Demo User \| Demo User's workspace \| personal=true \| owner`, created in the same transaction |
| Verification email | arrived in Mailpit **2 s** after signup (worker `SendEmail` job) |
| Token row | `verify_email` expires 1440 min (24 h) after issue |
| `POST /auth/verify-email` | `200` "email verified"; replay → `400` (single use) |

### US-02 — log in and stay logged in

| Step | Result |
| --- | --- |
| `POST /auth/login` | `200` |
| `sintade_session` | `Path=/; Max-Age=900; HttpOnly; Secure; SameSite=Lax` (15 min) |
| `sintade_refresh` | `Path=/api/v1/auth; Max-Age=2592000; HttpOnly; Secure; SameSite=Lax` (30 days) |
| `sintade_csrf` | `Path=/; Max-Age=2592000; Secure; SameSite=Lax` (readable by the SPA for double-submit) |
| `GET /me` | `200` user + `[{"role":"owner","is_personal":true,…}]` + `current_workspace_id` |
| Refresh without `X-CSRF-Token` | `403` |
| Refresh | `200`, refresh token value changed (rotation) |
| Replay the retired refresh token | `401` |
| The *new* token, after that reuse | `401`: the whole family was revoked |
| 6 wrong logins, one IP + email | `401 401 401 401 401 429` |

### US-03 — reset a forgotten password

| Step | Result |
| --- | --- |
| Forgot, known email / unknown email | both `202` with identical bodies |
| Token row | `password_reset` expires 60 min after issue |
| `POST /auth/password/reset` | `200`; replaying the token → `400` (single use) |
| Refresh token from before the reset | `401`: all sessions revoked |
| Login with old / new password | `401` / `200` |

### Day 19 — profile and logout everywhere

| Step | Result |
| --- | --- |
| `PATCH /me` `"  Demo Renamed "` | `200`, `display_name: "Demo Renamed"` (trimmed) |
| `PATCH /me` blank | `422` |
| `POST /auth/logout-all` | `204`; that session's refresh → `401` |

### Cross-cutting

- **Security headers** on every response: `strict-transport-security: max-age=31536000`,
  `x-content-type-options: nosniff`, `referrer-policy: strict-origin-when-cross-origin`,
  `content-security-policy: default-src 'self'; script-src 'self'; object-src 'none'; base-uri 'self'`,
  `permissions-policy: display-capture=(self)`, plus `x-request-id`.
- **Errors** are RFC 9457: anonymous `GET /me` → `401`, `application/problem+json`.
- **Tenant isolation:** `cargo test -p api tenant_isolation` → 4 passed. Every route is in
  `routes::table()`, and protected routes return `401` anonymously. Workspace routes hide
  other tenants' resources (`404`), and a removed member loses access on the next request.
- **Contract:** `just openapi-check` → no drift between the code, `docs/api/openapi.json`
  and `web/src/app/api/schema.ts`.

### Automated runs

- Local: `npx playwright test --project=chromium` → **4 passed** (capability matrix,
  signup→verify→login, wrong password, rename + log out everywhere).
- CI on `main` @ `0f7c171`, run
  [36229438335](https://github.com/oporoke/sintade/actions/runs/36229438335):
  **every job green, `audit` included** (the first fully green main run; the
  `rustls-webpki` advisories were fixed in PR #21). e2e **12 passed** across Chromium,
  Firefox and WebKit. Images for api/worker/web were built and pushed. `deploy-staging`
  was skipped (no VPS).
- `just check` / `just test`: green. 87 Rust tests, 29 Angular unit tests.

## Design checks

- **§15 acceptance criteria:** every criterion of US-01, US-02 and US-03 passes (evidence
  above), and each is ticked in `docs/design.md` with "verified locally".
- **§14 progress tracker:** the three Identity rows move from *Not started* to *In progress*,
  not *Done*, because the table's own rule requires "deployed" and nothing is deployed yet.
  The Hardening row is also *In progress*: rate limits, headers and tenant-isolation tests
  exist; backups and the production deploy do not.
- **§12 exit criteria:** these are MVP-wide (recording, stop→link, etc.). None apply yet.

## What did not happen

- **Staging demo.** There is no VPS, so the headline "new user signs up on staging" was
  demonstrated locally only.

## Carry-over

- **Staging VPS** (from M1, still open): `deploy-staging` is wired and waits on
  `vars.STAGING_HOST` plus secrets. When it exists, re-run section A against staging and
  mark the §14 Identity rows *Done*.
- Role → permission matrix is still `TODO: Verify` (ADR-0007); decide admin + billing
  before V1 teams.
- Logout-all and password reset leave already-issued access cookies valid for up to 15 min
  (stateless). Add a per-user session epoch if immediate revocation is needed.
- Profile: email change, password change while logged in, and an active-sessions list (V1).
- WebKit mixed-content check for HTTPS SPA → `http://localhost:9000` presigned PUTs, when
  uploads land (ADR-0006).
- Day 33 must add the first real workspace route to the tenant-isolation table and drop the
  `expect(dead_code)` markers.
