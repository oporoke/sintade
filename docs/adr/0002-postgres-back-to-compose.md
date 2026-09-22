# ADR-0002: Postgres runs via `compose.yml`, reverting ADR-0001

## Status

Accepted — 2026-09-22

## Context

[ADR-0001](0001-native-postgres-for-local-dev.md) had `compose.yml` provision MinIO and Mailpit
only, with Postgres run natively on this developer's machine (a pre-existing Postgres 18
cluster on port 5433) instead of in Docker, to avoid running two Postgres instances for one
project.

The user asked to revert this: run Postgres "as planned" — i.e. as documented in
`docs/design.md`, via `compose.yml` alongside MinIO and Mailpit, all three managed by
`just deps-up`.

## Decision

`compose.yml` adds a `postgres` service (`postgres:16`, matching `docs/design.md` and
`CLAUDE.md`'s "PostgreSQL 16+"), with a `pg_isready` healthcheck, a named volume, and the
standard port `5432`. `just deps-up`'s `--wait` group now includes `postgres` alongside `minio`
and `mailpit`, restoring the literal "all three containers healthy" Check from Day 2's plan.

`.env`/`.env.example`'s `DATABASE_URL` reverts to `postgres://app:app@localhost:5432/sintade`,
matching `docs/design.md`'s documented example exactly.

The native Postgres 18 cluster (port 5433) and the `sintade` database/`oporo` user created on it
are left in place, untouched and unused by the project going forward.

## Consequences

- `just deps-up` now starts and waits on all three services again, matching
  `docs/design.md` and the Day 2 Check as originally written.
- Port `5432` is free on this machine (confirmed via `ss -tlnp` before adding the service), so
  there's no conflict with the native cluster on `5433`.
- CI (Day 10) can rely on `compose.yml` (or its own service container) providing Postgres the
  same way it provides MinIO and Mailpit — no special-casing needed, unlike under ADR-0001.
