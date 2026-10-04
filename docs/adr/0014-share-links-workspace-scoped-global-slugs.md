# ADR-0014: Share links carry `workspace_id`; slugs are global, 12 base62 characters

## Status

Accepted — 2026-10-05 (Day 52, M6).

## Context

§8's `share_links` DDL has no `workspace_id`, yet CLAUDE.md rule 5 says every tenant-owned table
has one and every query filters by it. A viewer opening `/s/{slug}` has no workspace, so slug
lookup can't filter by one.

## Decision

1. `share_links` gains `workspace_id` (denormalised from the recording). Every management query
   (create, list, update, revoke) filters by it; a recording or link in another workspace is
   `404`. Lookup by slug is the one query that can't, and it is the point of the slug:
   `SharingService::resolve` is the only reader, and the viewer-facing routes (Day 53) decide
   access from the link's visibility.
2. A slug is **12 base62 characters** (71.4 bits, ≥ 70 per §16), drawn from the CSPRNG with
   rejection sampling (bytes ≥ 248 are dropped so `% 62` stays uniform). The column has a `UNIQUE`
   constraint and a shape `CHECK`; an insert that collides is redrawn (up to 5 times).
3. Revoking sets `revoked_at`; nothing caches links, so it takes effect on the next request.
   Revoked links cannot be edited. Expired links stop resolving; an expiry must be in the future
   when set.
4. `sharing` (L4) calls `catalog` (L3) to check the recording exists in the workspace, and writes
   `LinkCreated` to the outbox in the same transaction.
5. Passwords and invites are V1: `password_hash` stays `NULL`.

## Consequences

- A `workspace_id` mismatch between a link and its recording can't arise through the service
  (it writes both from the checked recording); a DB-level composite FK is a possible hardening.
- `visibility` is stored now; what each value permits is decided by `can_view` (Day 53).
