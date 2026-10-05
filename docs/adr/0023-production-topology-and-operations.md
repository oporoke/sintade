# ADR-0023: Production topology, deploy, error reporting and the watchdog

## Status

Accepted — 2026-10-05 (Day 69, M8).

## Context

§2 and §19 describe production as API host(s), a worker host and Postgres, deployed from a tagged
commit with a manual approval, with Sentry and alerts. No hosts, domain, DNS, Sentry project or
mail provider exist yet, so what can be built and proven today is everything that runs on them.

## Decision

1. **One compose file, three profiles** (`deploy/compose.prod.yml`): `edge` (Caddy, the web
   container, the API), `worker`, `data` (Postgres with WAL archiving, MinIO and its init). One
   host runs all three; a split starts only some per host. Caddy terminates TLS with automatic
   Let's Encrypt certificates for `DOMAIN` and `storage.DOMAIN` (browsers upload chunks to the
   second with presigned URLs, ADR-0010); the web container's nginx serves the SPA, proxies `/api`
   and the probes, and sets the page headers (ADR-0022). The app uses a MinIO *service account*,
   never the root user.
2. **Images are built once per `main` commit and tagged with its SHA; a release is a `vX.Y.Z` tag
   on such a commit.** `deploy-production` waits for the `production` environment's required
   reviewers, checks the three images exist, ships `deploy/` and the env files (from environment
   secrets, mode 0600) to the host and runs `deploy.sh <sha>`: pull → data services → migrate →
   worker → API (wait healthy) → web/edge → smoke test through TLS. The failure message prints the
   rollback command. Config and images always come from the same commit.
3. **Migrations are embedded** in the API binary (`api migrate`; `platform::migrate`), so the
   image migrates its own database with no extra tooling, from one place at a time (concurrent
   runs can deadlock on `CREATE INDEX CONCURRENTLY`; found by test).
4. **Sentry** (`SENTRY_DSN`, optional) receives `ERROR`-level events only, with request details,
   users, server name and breadcrumbs removed before sending and personal data off, so nothing
   the logging rules keep out of logs (emails, tokens, signed URLs) can leave through it.
5. **The `OpsWatchdog` job** (worker, every 5 minutes) replaces a metrics stack at this size: it
   reads the queue, recordings and the backup archiver and raises `ALERT <kind>` errors (Sentry)
   and, with `ALERT_EMAIL`, one email per kind per hour: queue backlog (> 10 min), dead jobs,
   recordings stuck in `processing` (> 90 min), failing WAL archiving. An external uptime check on
   `/readyz` covers what cannot report itself (the API, SMTP).
6. **What a local run proves.** `deploy/smoke-local.sh` (`just prod-smoke`) runs the real images
   through `deploy.sh` with Caddy's internal CA and checks the probes, headers, deep links, a
   sign-up and a chunk uploaded to the storage host and acknowledged.

## Consequences

- Provisioning is the only manual work and is written down (`docs/runbooks/provision-host.md`).
- Rollback is "deploy the previous SHA"; the database is never rolled back.
- Open: domain, VPS and off-site providers, SMTP provider, Sentry project, CDN (and then real
  client IPs for rate limits). All `TODO: Verify` until decided.
