# ADR-0029: Live status over SSE, fed by the outbox

## Status

Accepted — 2026-10-06 (Day 82, M10). Fills in docs/design.md §5 "SSE hub" and §9
`GET /recordings/{id}/events`; adds one crate dependency (§2 does not name it).

## Context

The watch page asked "is it ready yet?" on a timer (ADR-0022 budgets those requests) and the
library didn't update at all until reloaded. §5 and §9 already call for Server-Sent Events fed by
Postgres `LISTEN/NOTIFY`.

## Decision

1. **One hub per API process** (`bin/api/src/status_hub.rs`). It `LISTEN`s on the outbox channel.
   A `NOTIFY` carries only the event type, so on every notification — and every 5 s regardless,
   and after any reconnect — it reads the new `outbox_events` rows by id and publishes a
   `Change { workspace_id, recording_id }` for `RecordingReady`, `ProcessingFailed` and
   `RenditionReady` on a `tokio::sync::broadcast` channel. A lost notification therefore costs at
   most 5 s, never a missed change. The listener starts when the first stream opens, so an API
   process nobody is following holds no extra database connection.
2. **A stream sends state, not deltas.** On connect (after subscribing, so nothing slips between)
   and after every change to its recording, the stream re-reads the recording's status
   (`state`, and whether the `hls` ladder, `sprite` and `preview` exist) and sends it as a
   `status` event only if it differs from the last one sent. The browser's `EventSource` needs no
   merge logic and a reconnect is just another first frame. A trashed or deleted recording sends
   `gone` and ends.
3. **Two endpoints.** `GET /api/v1/recordings/{id}/events` (a `Workspace` route: the caller's
   workspace only, `404` otherwise, row in the tenant-isolation table) and
   `GET /api/v1/s/{slug}/events` (a `Viewer` route: the same access decision as `/playback`).
   Streams close after 15 minutes so a session or link revoked meanwhile is noticed on the
   reconnect.
4. **The client falls back.** The watch page polls as before when a stream can't be opened (no
   `EventSource`, refused, server gone); the library follows only recordings that aren't ready.
5. **Dependency.** `tokio-stream` (the Tokio project's `Stream` adapters; already in `Cargo.lock`
   through other crates) in `bin/api`, because `axum::response::sse::Sse` takes a `Stream` and
   nothing in the tree gave us one.
6. **`RenditionReady` carries `variants`** (the ladder's rungs, lowest first) rather than the
   single `variant` the §9 table shows, so one event announces one finished job.

## Consequences

- No per-recording polling from the watch page while a stream is up.
- Each open stream is a task and a database read per change to its recording, not per second.
  Streams are not yet capped per process (`TODO: Verify` against the Day 87 load test).
- A multi-instance API needs nothing extra: every instance has its own hub on the same channel.
