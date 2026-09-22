# ADR-0003: Rename the `core` crate to `kernel`

## Status

Accepted — 2026-09-22

## Context

`docs/design.md`'s backend workspace layout (and Day 1's scaffold) named the shared-kernel crate
`core`, at `crates/core`. While wiring Day 3's `platform`/`bin/api` (adding `#[tokio::main]` and
`#[sqlx::test]`), the build broke with errors that only appear when a crate literally named
`core` is anywhere in the dependency graph:

```
error[E0433]: cannot find `future` in `core`
  --> bin/api/src/main.rs:3:1
   |
3  | #[tokio::main]
```

```
error: cannot find `prelude` in `core`
```

Cause, confirmed by removing the local `core` dependency and rebuilding successfully: naming a
crate `core` shadows the language's own `core` crate for any macro-generated code that emits an
unqualified `core::...` path (rather than the fully-qualified `::core::...`). `tokio::main` and
`sqlx::test` both do this. This isn't specific to our code or toolchain version — it's a known
footgun of naming any crate exactly `core`, `std`, or `alloc`, and it will keep recurring
unpredictably as more macro-heavy dependencies are added over the remaining 335 days, not just
today.

## Decision

Rename the crate from `core` to `kernel` — directory `crates/core` → `crates/kernel`, package
name and lib name `core` → `kernel`. Chosen to match `docs/design.md`'s own description of the
crate's responsibility ("Shared kernel types and the event envelope"). No source code changes
were needed beyond `Cargo.toml`/workspace-member paths, since the crate was still an empty stub
(Day 4 is the first day that adds real content to it).

`docs/design.md` is left as written (it is the source-of-truth spec, not something this ADR
edits); this ADR is the record of the deviation. `CLAUDE.md`'s layering rule ("L0 `core` → L1
`platform` → …") was updated to say `kernel` instead, since it's read as live instructions every
day and an unqualified real bug, not a design preference, is not worth preserving as a stale
name.

## Consequences

- Every future day that would have said `crates/core` in its deliverable instead uses
  `crates/kernel`; `kernel::` is the import path for shared-kernel types (`UserId`,
  `WorkspaceId`, `AppError`, `DomainEvent`, `Entitlements` — Day 4 onward).
- Any other crate name matching a standard-library crate (`core`, `std`, `alloc`, `proc_macro`,
  `test`) should be avoided for the same reason if it comes up later in the plan.
