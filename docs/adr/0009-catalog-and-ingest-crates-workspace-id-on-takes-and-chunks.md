# ADR-0009: `catalog` + `ingest` crates for `POST /recordings`; `workspace_id` on `takes` and `chunks`

## Status

Accepted — 2026-09-28 (Day 33).

## Context

Day 33 adds the first media tables and `POST /api/v1/recordings`. `docs/design.md` §9 lists
the route as "catalog + ingest": `catalog` owns `recordings`, `ingest` owns `takes`,
`upload_sessions` and `chunks` (§5). Three questions came up:

1. **Atomicity across crates.** The recording and its take must be created together, but each
   crate's `infra/` may only touch its own tables (CLAUDE.md rule 3). `catalog` is L3 and
   `ingest` is L4, so `ingest → catalog` is an allowed downward edge.
2. **Tenant scoping of `takes` and `chunks`.** §8's schema gives them no `workspace_id`; they
   reach the tenant only through `recordings`. CLAUDE.md rule 5 (non-negotiable) says every
   tenant-owned table has `workspace_id` and every query filters by it.
3. **`upload_sessions`.** §8's diagram has `TAKES ||--|| UPLOAD_SESSIONS`, and §9 says
   `POST /recordings` creates an upload session, but §8 defines no columns for it. README
   already marks the table `TODO: Verify`.

## Decision

1. `crates/catalog` (L3) and `crates/ingest` (L4) are created. `IngestService::start_recording`
   owns the transaction and calls `CatalogService::create_recording` on its connection, the
   same pattern as `TenancyService::create_personal_workspace` inside registration
   (ADR-0007). The IDs are generated first, so the recording is inserted with `current_take`
   already set and the take then references it.
2. **`takes` and `chunks` get a `workspace_id NOT NULL REFERENCES workspaces(id)`**, set to the
   recording's workspace. It is denormalised on purpose: chunk presign/ack/status (Days 34–36)
   are hot paths keyed by `take_id`, and they can filter `WHERE take_id = $1 AND workspace_id =
   $2` without joining `recordings`. `chunks` also gets `CHECK`s on `idx >= 0`, `size_bytes >
   0` and a 32-byte `sha256`.
3. **`upload_sessions` is deferred.** Nothing in Days 33–36 needs state beyond `takes`
   (`finalized_at`) and `chunks`. If a later day needs per-session state (e.g. Day 41's
   `SweepStaleUploads`), it adds the table with columns chosen then, recorded in its own ADR.
4. `POST /recordings` requires `Permission::CreateRecording` (a viewer gets `403`) and the CSRF
   double-submit token. It creates in the **session's** workspace; the body names no
   workspace.
5. The tenant-isolation harness gains a `Probe::Create` kind. A create route has no
   A-owned resource for B to aim at, so the harness creates as B and as A and asserts each row
   landed in its creator's workspace and never in the other's.

## Consequences

- §8's SQL sketch and the tables differ by these columns. §8 is a sketch; this ADR and the
  migrations are the record.
- Every future query on `takes`/`chunks` must filter by `workspace_id` as well as the ID.
- The design's "creates … upload session" wording for `POST /recordings` is not yet true;
  README says so.
