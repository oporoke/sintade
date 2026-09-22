# Sintade — Build Progress

## Current day

**Day 2** — `compose.yml` with Postgres, MinIO (bucket + CORS), Mailpit; `.env.example`; `just deps-up`

## Current milestone

M1 — Foundation (Days 1–10)

## Log

| Day | Date | Deliverable | Status | PR/commit | Evidence | Notes |
| --- | --- | --- | --- | --- | --- | --- |
| 1 | 2026-09-22 | Cargo workspace (`core`, `platform`, `bin/api`, `bin/worker`), `rust-toolchain.toml`, `justfile`, README status update | Done | `feat/day-001-cargo-workspace-scaffold` | `cargo build --workspace` exit 0; `just check` (fmt --check + clippy -D warnings) exit 0; `just test` exit 0 (0 tests, all crates are stubs) | `bend/`/`fend/` were removed from the working tree (moved to OS trash outside this session, before work started); their removal is included in this commit, resolving the earlier layout conflict with `docs/design.md` |

## Deferred items

- None yet.

## Known issues / open items

- The Angular app (`web/` per `docs/design.md`) does not exist yet. The previous `fend/` scaffold was removed (see Day 1 log) before its content was reviewed, so Day 9 (Angular shell) starts from a fresh `ng new`, not from `fend/`'s history. If a copy of that scaffold is wanted, it is recoverable from `~/.local/share/Trash` on this machine until emptied.
