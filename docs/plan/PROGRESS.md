# Sintade — Build Progress

## Current day

**Day 3** — `platform`: config loader, `PgPool`, JSON `tracing`, sqlx migration wiring; API serves `/healthz`, `/readyz`

## Current milestone

M1 — Foundation (Days 1–10)

## Log

| Day | Date | Deliverable | Status | PR/commit | Evidence | Notes |
| --- | --- | --- | --- | --- | --- | --- |
| 1 | 2026-09-22 | Cargo workspace (`core`, `platform`, `bin/api`, `bin/worker`), `rust-toolchain.toml`, `justfile`, README status update | Done | `feat/day-001-cargo-workspace-scaffold` | `cargo build --workspace` exit 0; `just check` (fmt --check + clippy -D warnings) exit 0; `just test` exit 0 (0 tests, all crates are stubs) | `bend/`/`fend/` were removed from the working tree (moved to OS trash outside this session, before work started); their removal is included in this commit, resolving the earlier layout conflict with `docs/design.md` |
| 2 | 2026-09-22 | `compose.yml` (MinIO + Mailpit), `.env.example`, `just deps-up` | Done | `feat/day-002-deps-compose` | `docker compose ps`: `minio` and `mailpit` both `Up ... (healthy)`; `minio-init` job created bucket `sintade-dev` (`Bucket created successfully`); CORS preflight `OPTIONS` on the bucket returns `Access-Control-Allow-Origin: http://localhost:4200`, `Allow-Methods: PUT`; `just check`/`just test` still green | Postgres was initially left out of `compose.yml` in favor of a native cluster (ADR-0001); reverted same day per user request (ADR-0002) — see follow-up row below. MinIO images pulled from `quay.io/minio/minio` (Docker Hub's `minio/minio` was archived Oct 2025); bucket-CORS via `mc cors` is AIStor-only (paid), so CORS is set server-wide via `MINIO_API_CORS_ALLOW_ORIGIN` instead. `quay.io/minio/mc` hit a flaky CDN edge (TLS handshake timeouts); worked around by reusing the already-pulled `quay.io/minio/minio` image for the bucket-init step, since it bundles the `mc` binary too |
| 2 (follow-up) | 2026-09-22 | Revert ADR-0001: `postgres:16` added back to `compose.yml`, `just deps-up`, `.env`/`.env.example` DATABASE_URL back to `localhost:5432` | Done | `feat/day-002-postgres-in-compose` | `docker compose ps`: `postgres`, `minio`, `mailpit` all `Up ... (healthy)` — literal three-container Check restored; `psql -h localhost -p 5432 -U app -d sintade` connects; `just check`/`just test` still green | ADR-0001 marked superseded by [ADR-0002](../adr/0002-postgres-back-to-compose.md). The native Postgres 18 cluster (port 5433, `sintade` db, `oporo` user) is left in place, untouched and now unused by the project |

## Deferred items

- None yet.

## Known issues / open items

- The Angular app (`web/` per `docs/design.md`) does not exist yet. The previous `fend/` scaffold was removed (see Day 1 log) before its content was reviewed, so Day 9 (Angular shell) starts from a fresh `ng new`, not from `fend/`'s history. If a copy of that scaffold is wanted, it is recoverable from `~/.local/share/Trash` on this machine until emptied.
- MinIO's official free Docker images moved from Docker Hub to `quay.io/minio/*` (Oct 2025 policy change); `docs/design.md`'s "MinIO bucket CORS" line doesn't apply to the free/community edition (per-bucket CORS is AIStor/paid-only) — CORS is configured server-wide via `MINIO_API_CORS_ALLOW_ORIGIN` instead, which is equivalent for a single-bucket dev setup.
