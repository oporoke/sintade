# M1 — Foundation demo (Days 1–10)

Date: 2026-09-24
Commit: `main` @ `e17c48b` (Day 10 merge + follow-up doc commit)

## Scope

M1 is infrastructure, not a user-facing feature. Per `docs/design.md`'s roadmap (§12, weeks
1–2), the deliverable is: "Cargo workspace, `core`, `platform` (config, pool, ObjectStore on
MinIO, JobQueue, outbox, tracing); Angular shell; CI; Docker Compose dev env." There is no
recording/upload/playback yet — that starts at Capture (weeks 5–7) and Ingest (weeks 7–8). This
demo walks the foundation end to end: dev environment up, API healthy, a real job processed
through the queue and outbox, an email actually delivered, the Angular shell rendering real
browser-capability detection, and CI enforcing all of it plus building deployable images.

## Steps and results

### 1. Local environment

```
just deps-up
docker compose ps
```

Result: `postgres`, `minio`, `mailpit` all report `Up ... (healthy)`.

### 2. Migrations

```
just db-migrate
```

Result: `jobs` and `outbox_events` tables exist (from Day 6 and Day 7's migrations).

### 3. API health and error handling

```
just api          # in one terminal
curl -i http://localhost:8080/healthz
curl -i http://localhost:8080/readyz
curl -i http://localhost:8080/nope
```

Result (captured 2026-09-24T11:55Z):
- `/healthz` → `200 OK`, `x-request-id` header present
- `/readyz` → `200 OK` via a real `SELECT 1` through the connection pool
- `/nope` → `404`, `content-type: application/problem+json`, body
  `{"type":"about:blank","title":"Not Found","status":404,"detail":"not found"}`

### 4. Job queue → outbox → email, end to end

```sql
INSERT INTO jobs (id, kind, payload)
VALUES (gen_random_uuid(), 'SendEmail', '{"to":"demo@example.test","subject":"M1 demo","body":"foundation is solid"}');
```

```
just worker
```

Result: worker log shows `"send_email job ran"`; the job row's `done_at` is set; the message
appears in Mailpit (`http://localhost:8025`) with the correct from/to/subject, confirmed via
Mailpit's REST search API.

(The outbox relay and `SKIP LOCKED`/backoff/DLQ mechanics were proven with dedicated integration
tests on Days 6–7 — nothing in the product yet emits domain events for this demo to show live,
since no feature module exists to publish them.)

### 5. Angular shell

```
just web          # http://localhost:4200/debug
```

Result: the `/debug` page renders a table with `getDisplayMedia`, `mediaRecorderWebm`, `opfs`,
`systemAudio`, each showing a real (not stubbed) detection result for the browser it's opened
in. Verified via `npx playwright test --project=chromium` locally (passed) and via GitHub
Actions across all three engines — chromium, firefox, webkit (see CI run below).

### 6. `just check` / `just test`

Both green: `cargo fmt --check`, `cargo clippy -D warnings`, `cargo sqlx prepare --check`,
`npm run lint` (check); `cargo test --workspace` (15 tests) + `npm test` (3 test files) (test).

### 7. CI and images

PR #11 (`feat/day-010-ci-images`) — GitHub Actions run
[35992881633](https://github.com/oporoke/sintade/actions/runs/35992881633): `backend`, `web`,
`e2e` (all three browsers) pass; `audit` reports 4 real transitive-dependency advisories,
informationally (`continue-on-error`); overall conclusion `success`.

Push to `main` — run
[35994060836](https://github.com/oporoke/sintade/actions/runs/35994060836): `build-images`
matrix (api, worker, web) all succeed, pushing real images to
`ghcr.io/oporoke/sintade/{api,worker,web}` tagged by commit SHA. `deploy-staging` correctly
reports `skipped` (see Carry-over).

## Design.md checks

- §12 MVP exit criteria (30-min cross-browser recording, crash recovery, stop→link ≤ 5s, etc.)
  are not applicable yet — they require Capture, Ingest and Processing, none of which exist
  until later milestones. None ticked; none expected to be.
- §15 acceptance criteria: no MVP user story exists for infrastructure, so nothing to check
  against here either — the first applicable stories (US-01..03, Identity) start at M2.
- §14 progress tracker: updated (see below) — the three MVP rows this milestone covers moved
  from "Not started" to "In progress" (not "Done": design.md's own rule states Done requires
  "merged to `main`, deployed, and its acceptance criteria pass" — nothing is deployed to a
  real environment yet).

## Carry-over into M2 and beyond

- **No staging VPS exists.** `deploy-staging` (CI) is fully wired (SSH deploy, `sqlx migrate
  run` against `STAGING_DATABASE_URL`, `/readyz` smoke test) but gated on `vars.STAGING_HOST`
  being set. This was confirmed with the user before Day 10 was built (see
  `docs/plan/PROGRESS.md`). Day 10's literal Check ("staging shell loads, `/readyz` green")
  is not met until a VPS + secrets exist.
- `cargo audit` reports RUSTSEC-2026-0258 (`h2` 0.3.27), -0098/-0099/-0104 (`rustls-webpki`
  0.101.7), all in transitive dependencies. Informational for now; revisit at Day 14
  (Hardening) by upgrading whichever crates pull these old versions in.
- Per-kind job concurrency limiting (`WORKER_CONCURRENCY`) is defined but unused — needs a
  second real job kind to justify building it.
