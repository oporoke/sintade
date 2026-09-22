# ADR-0001: Postgres runs natively for local development, not via `compose.yml`

## Status

Superseded by [ADR-0002](0002-postgres-back-to-compose.md) — 2026-09-22

## Context

`docs/design.md` ("Local development") documents `just deps-up` as bringing up Postgres, MinIO
and Mailpit together via `compose.yml`, all three as Docker containers.

On this developer's machine, a native (non-Docker) Postgres 18 cluster was already running
(`pg_lsclusters` shows cluster `18/main` on port 5433), with a `sintade` database and `oporo`
user created directly on it before Day 2 work started. Running a second, Dockerized Postgres
alongside it would mean two Postgres instances for one project, for no benefit, on a
single-developer machine.

## Decision

`compose.yml` provisions **MinIO and Mailpit only**. Postgres is expected to already exist
locally (native install or any other method a developer chooses) and is configured via
`DATABASE_URL` in `.env`. `.env.example`'s `DATABASE_URL` points at `localhost:5433` (this
machine's native cluster's port) with placeholder credentials, and carries a comment pointing
back to this ADR.

Day 2's Check ("all three containers healthy") is satisfied by the two Docker services
(`minio`, `mailpit`) reporting healthy; Postgres reachability is verified separately via `psql`,
not via `docker compose ps`.

## Consequences

- `just deps-up` no longer starts Postgres. Anyone following `docs/design.md`'s literal "First
  run" instructions verbatim on a different machine needs their own Postgres 16+ instance
  reachable at the `DATABASE_URL` they configure — this is unchanged from before in spirit
  (`.env` was always machine-specific), but `compose.yml` no longer provides it for them.
- CI (Day 10) will need its own Postgres — most likely a GitHub Actions service container or a
  `compose.yml` override — since there is no native cluster on a CI runner. This is deferred to
  Day 10 and logged in `docs/plan/PROGRESS.md`.
- If a second developer joins, or this project moves to a machine without a suitable native
  Postgres, `compose.yml` can add a `postgres` service back at that point; this ADR does not
  preclude it.
