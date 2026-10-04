# ADR-0012: Media processing — the chunk manifest travels in `TakeFinalized`, `media_jobs`, lock heartbeats

## Status

Accepted — 2026-10-04 (Day 43, M5).

## Context

M5 builds `ProcessTake` (docs/design.md §10 Process): download a take's chunks, verify their
SHA-256, concatenate, validate with ffprobe, produce a fast-start MP4 and a poster, mark the
recording `ready` or `failed`. Several things weren't settled:

- `media` (L4) must not depend on `ingest` (L4) — CLAUDE.md rule 2 allows only events between
  them — yet processing needs the take's chunk list (keys, sizes, hashes), which lives in
  ingest's `chunks` table. §4 lists `TakeFinalized { take_id, chunk_count, mime }`.
- A manual retry of a failed recording (Day 48) needs the same list again, long after the
  event was dispatched.
- The outbox relay delivers at least once, so a `TakeFinalized` can arrive twice.
- Jobs hold a 30 s lock (Day 6) and the worker runs them one at a time; transcoding a
  30-minute take takes minutes, so another worker would reclaim it mid-run.
- §6's table says media → catalog goes through events, but ingest already moves the recording
  to `processing` by calling `catalog` (L3) directly inside its transaction (Day 36).

## Decision

1. **`TakeFinalized` carries the chunk manifest**: `chunks: [{ idx, key, size_bytes, sha256 }]`
   for `0..chunk_count`, read from `chunks` in the finalize transaction. Media deserialises its
   own view of the event (`TakeFinalizedMessage`), so the two crates share a JSON contract, not
   code. A 30-minute take is ~900 entries (~100 KB of JSON), acceptable for `outbox_events` and
   `jobs` rows.
2. **`media_jobs` (owned by media)** stores one row per take: the manifest, MIME type,
   duration, state (`queued` → `running` → `done`/`failed`), attempts and last error. Its
   primary key is `take_id`, so the subscriber is idempotent: a replayed event inserts nothing
   and enqueues nothing. The row and its `ProcessTake` job are written in one transaction
   (`JobQueue::enqueue_in`). The job payload is just `{ take_id, workspace_id }`; a retry
   re-enqueues from the row.
3. **`renditions` gets `workspace_id`** (as ADR-0009 did for takes and chunks) so every query
   filters by tenant.
4. **Lock heartbeat**: while a job runs, the worker renews its lock every 10 s
   (`JobQueue::extend_lock`, only if it still holds it). A crashed worker's job is reclaimed
   ≤ 30 s after its last heartbeat.
5. **Scratch space**: each job gets `WORKER_SCRATCH_DIR/<take_id>/`, recreated empty at the start
   and removed when the job ends, by `ScratchDir::remove` or its `Drop` on panic/cancel. The
   worker sweeps directories older than 6 h at startup.
6. **Media calls `catalog` directly for state changes** (L4 → L3 is allowed), like ingest
   does, so the recording's state, its rendition rows and the `RecordingReady` /
   `ProcessingFailed` outbox event commit together. The events remain the integration point
   for messaging (the "ready" email) and later subscribers.
7. **Failures are classified.** Bad input — a chunk whose bytes don't match its hash, a source
   ffprobe rejects — fails the recording at once (`failed`, `ProcessingFailed { reason }`, the job
   completes: retrying can't help). Infrastructure trouble — storage, scratch disk, an FFmpeg
   timeout — fails the attempt; the queue retries with backoff, and the last attempt marks the
   recording `failed`.

## Consequences

- Takes finalized before Day 43 (local dev data only) have no `media_jobs` row; their events
  were dispatched with no subscriber (Day 36 deferred note).
- A change to the manifest's shape is a contract change between ingest and media: both sides'
  tests pin it (`finalize_moves_the_recording_to_processing_and_emits_take_finalized`,
  media's `TakeFinalizedMessage` tests).
- §6's "media → catalog: events" row is superseded for state changes by decision 6.
