# ADR-0011: Free-tier entitlements (402), the recorder stops at the limit, and the stale-upload sweep

## Status

Accepted — 2026-09-28 (Day 41).

## Context

Day 41 enforces the free tier decided in §13 (50 recordings, 10 minutes each, up to 1080p) and
adds `SweepStaleUploads` (§10 Recover step 4: abandon uploads idle for more than 24 h, delete
their chunks after 7 days). Some choices weren't settled:

- README's API table marked the entitlement status as "402/403 `TODO: Verify`".
- A take can reach 10 minutes long before it's finalized. Rejecting it only at finalize
  would leave a finished recording the server refuses.
- The recording count must hold under concurrent creates.
- `ingest` owns takes and chunks, and `catalog` owns recording state. The sweep needs both.

## Decision

1. **`crates/billing` (L3)** is a stub: `BillingService::entitlements(workspace)` returns
   `FREE_TIER` for every workspace (`kernel::Entitlements { max_recordings, max_duration_ms,
   max_resolution, seats }`). V1 swaps in plans and subscriptions behind the same method (KI-006).
2. **Plan limits are `402 Payment Required`** (problem+json; `AppError::LimitReached`). `403`
   stays "your role can't" and `422` stays "your input is wrong". A limit is neither: it can be
   lifted by a plan.
3. **Create:** `IngestService::start_recording` counts the workspace's recordings that aren't
   abandoned or trashed (`CatalogService::lock_and_count_active`) under a transaction-scoped
   advisory lock per workspace, so concurrent creates can't overshoot.
4. **Finalize:** a take longer than `max_duration_ms` is `402`; exactly the limit is accepted.
5. **The recorder stops itself** 1 s before the limit. `POST /recordings` returns
   `max_duration_ms` so the client knows it. If creating is refused with `402`, the recorder
   says so and doesn't start. It does not fall back to recording on the device, which it does
   only when the server can't be reached.
6. **The sweep** runs hourly as the `SweepStaleUploads` job. Each worker schedules it with
   `JobQueue::enqueue_unless_pending`, so there is only one copy.
   - `ingest` marks unfinalized takes idle for more than 24 h (no chunk acked, no take created
     since) with a new `takes.abandoned_at`. That way each run moves on to new ones.
   - `catalog` moves their recordings from `recording`/`uploading` to `abandoned`, in the same
     transaction.
   - Abandoned takes idle for more than 7 days lose their chunk objects (a prefix delete) and
     rows.
7. **Recovery after abandonment:** finalizing an abandoned recording is `409`. The client's
   recovery then uploads the device's copy as a new recording.

## Consequences

- The sweep is a system job. It spans workspaces by design, and every row it touches is
  addressed by its own `workspace_id`.
- Offline-recorded takes don't know the limit. A recovered one longer than 10 minutes is
  refused (`402`, shown in the recovery dialog) and stays on the device.
- `max_resolution` isn't enforced yet: the recorder captures at up to 1080p by default, and the
  server can't see resolution before processing (M5 `ffprobe`).
