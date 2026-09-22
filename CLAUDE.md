# Sintade — Claude Code project instructions

Sintade is a browser-based screen recording platform: record screen, mic, system audio and webcam in the browser; upload chunks while recording; process to MP4/HLS; share a link seconds after stop. First segment: developers and tech teams in Tanzania. Plans in TZS, paid via direct mobile money APIs.

## Sources of truth (read before acting)

| File | What it is |
| --- | --- |
| `docs/design.md` | Product & Architecture Document. Sections are referenced as §1–§22 |
| `docs/plan/daily-build-plan.md` | Days 1–335, one deliverable + one check per day |
| `docs/plan/PROGRESS.md` | Current day, milestone, log, deferred items. **Always read first** |
| `README.md` | Master README; status markers must match reality |
| `docs/adr/` | Architecture decision records |

If code and docs disagree, stop and ask. Never silently change a decision recorded in §13 (decision log) or an ADR; propose a new ADR instead.

## Stack

Rust (stable, `rust-toolchain.toml`) · Axum/Tokio · sqlx (compile-checked) · PostgreSQL 16+ · MinIO (S3 API) · FFmpeg/ffprobe · whisper.cpp (V1) · Angular (standalone, signals, OnPush) · Manifest V3 extension · Docker Compose · GitHub Actions · `just`.

## Commands

```bash
just deps-up      # postgres, minio, mailpit
just db-migrate   # sqlx migrate run
just seed         # demo data
just api          # cargo run -p api      -> :8080
just worker       # cargo run -p worker
just web          # Angular              -> :4200
just check        # fmt --check, clippy -D warnings, sqlx prepare --check, npm run lint
just test         # Rust unit + integration, Angular unit
just e2e          # Playwright: Chromium, Firefox, WebKit
just migration <name>
just reset        # local DB + bucket only
```

If a command in this list does not exist yet, the day that introduces it must add it to the `justfile`.

## Architecture rules (non-negotiable)

1. **Modular monolith.** One crate per bounded context under `crates/`; binaries `bin/api`, `bin/worker`.
2. **Layering.** L0 `kernel` → L1 `platform` → L2 `identity`, `tenancy` → L3 `catalog`, `billing` → L4 features → L5 binaries. A crate depends only on lower layers. Allowed L4 edges: `delivery→sharing`, `delivery→media`, `engagement→sharing` via traits. Everything else between L4 crates goes through outbox events. (The L0 crate lives at `crates/kernel`, not `crates/core` — see [ADR-0003](docs/adr/0003-rename-core-crate-to-kernel.md): naming a crate `core` breaks macro-generated code that emits unqualified `core::` paths, e.g. `#[tokio::main]`.)
3. **Crate shape.** `domain/` (no async, no sqlx, no I/O) · `app/` (service trait + impl, owns transactions) · `infra/` (sqlx, only this crate's tables) · `events.rs` · `lib.rs` exports service trait, DTOs, events only.
4. **Media never touches the API process.** Browser → presigned PUT → MinIO → worker → MinIO → CDN.
5. **Tenant isolation.** Every tenant-owned table has `workspace_id`; every query filters by it. Inaccessible private resources return `404`, not `403`. Every new route is added to the tenant-isolation test table.
6. **Chunks are the source of truth** until processed. Client writes to OPFS before uploading. Chunk ack is idempotent on `(take_id, idx)`; different hash → `409`.
7. **Postgres is the queue.** `jobs` with `SKIP LOCKED`; domain events via `outbox_events` written in the same transaction as state changes.
8. **Ports and adapters** for storage, email, payments, transcription, AI.
9. **`capture/` is framework-free TypeScript** so the extension reuses it unchanged.

## Code conventions

- Rust: `cargo fmt`; `clippy -D warnings`; `#![deny(clippy::unwrap_used)]` in library crates; newtype IDs (UUIDv7) across boundaries; `thiserror` per module, `anyhow` only in binaries/tests; `sqlx::query!`/`query_as!` only; `#[tracing::instrument(skip_all, fields(...))]` on service methods; UTC via injected `Clock`.
- Errors: module error → `AppError` at handler → RFC 9457 problem+json.
- Never log emails, passwords, tokens, presigned URLs, secrets.
- TypeScript: `strict`, no `any`, DTOs generated from OpenAPI (utoipa), all strings via `$localize`.
- Naming: tables plural snake_case; `_at` timestamps; events PastTense (`TakeFinalized`); jobs verb-noun (`ProcessTake`); REST plural kebab-case under `/api/v1`.
- Migrations: forward-only, one concern, module-prefixed; never edit an applied migration; `CREATE INDEX CONCURRENTLY` in its own file.
- Commits: Conventional Commits. Branches: `feat/day-NNN-<slug>`.

## How a working day runs

Use `/day` to start a day, `/status` to see where things stand, `/demo` on a milestone's last day.

A day is **done** only when:
1. The day's deliverable exists and its **Check** column passes, with evidence (command output, test name, screenshot path).
2. `just check` and `just test` are green (and `just e2e` if the day touched capture, upload or playback).
3. New routes are in the tenant-isolation table; new behaviour has tests (bug fixes: failing test first).
4. `README.md` status markers and any affected docs are updated; decisions have an ADR.
5. `docs/plan/PROGRESS.md` is updated and the work is merged to `main`.

## Guardrails

- Do **only** the current day's deliverable. If you notice later-day work, note it under "Deferred" in PROGRESS.md; do not build it.
- Yesterday's merged build is the baseline. If it is broken, fixing it is today's first task and is logged.
- If the plan is ambiguous or a day is too large, say so and propose a split before coding; the plan shifts, it never skips.
- Never commit secrets; use `.env` locally and placeholders in `.env.example`.
- Never weaken a security control, rate limit, or test to make something pass.
- Ask before adding a new dependency that is not already implied by §2 (tech stack); prefer the standard library and existing crates.
- Mark anything unknown as `TODO: Verify` rather than inventing it.

## Key targets (§11)

Stop → link ≤ 5 s p95 · MP4 ready ≤ 0.5× duration · API p95 ≤ 100 ms · first frame ≤ 1.5 s p75 · client CPU ≤ 25% at 1080p30 · zero lost recordings.
