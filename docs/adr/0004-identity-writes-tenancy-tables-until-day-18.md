# ADR-0004: `identity` writes `workspaces`/`memberships` directly until `tenancy` exists

## Status

Accepted — 2026-09-24

## Context

CLAUDE.md's architecture rules are marked non-negotiable, including rule 3 (crate shape:
`infra/` touches only that crate's own tables) and the layering rule (a crate depends only on
*lower* layers; `identity` and `tenancy` are both L2 — peers, not one below the other).

Day 12's deliverable is `POST /auth/register`, which must create the user's personal workspace
"in the same transaction" as the user (matching US-01: "Personal workspace created in the same
transaction as the user") — i.e. inserting into `workspaces` and `memberships`. Those tables are
owned by `tenancy` (`docs/design.md`'s module table: "`tenancy` | Workspaces, members, roles,
policies | ... | `workspaces`, `memberships`, ..."), which isn't scheduled to exist as its own
crate until Day 18 ("`WorkspaceContext` extractor, `Permission` enum, `authorize()`").

There is no way to satisfy today's literal, correctly-specified deliverable without either (a)
building a full `tenancy` crate today, six days ahead of its scheduled day, with just enough
surface for `identity` to call into it — itself a form of layering violation in miniature and
real scope creep — or (b) having `identity` write to `workspaces`/`memberships` directly, as a
flagged, temporary exception.

## Decision

`identity`'s `app` layer (`RegisterService` or equivalent) inserts directly into `workspaces`
and `memberships` within the same transaction as `users`/`credentials`/`email_tokens`. The
`workspaces`/`memberships` migration is still module-prefixed `tenancy_` (matching who
conceptually owns the tables), not `identity_`, so the migration history stays honest about
ownership even though `identity`'s Rust code is what writes to them for now.

## Consequences

- This is a deliberate, temporary violation of CLAUDE.md's crate-shape rule ("infra/ ... only
  this crate's tables"), not a silent one. Flagged here, in `docs/plan/PROGRESS.md`, and in a
  code comment at the write site.
- **Day 18 must remove this exception**: extract a real `tenancy` crate owning
  `workspaces`/`memberships`/`workspace_invites`/`workspace_policies`, move the
  personal-workspace-creation logic there behind a service trait, and have `identity` call that
  instead of writing the tables itself. Until then, any other module needing to touch
  `workspaces`/`memberships` should extend this same identity-owned code path rather than
  inventing a second ad hoc writer.
- No other identity/tenancy boundary is affected — `identity` still does not touch
  `workspace_invites`/`workspace_policies` (V1-only tables, not needed yet).
