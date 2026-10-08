# ADR-0034: A short in-memory cache on the playback path

## Status

Accepted — 2026-10-08 (Day 87, M10). Implements the "caches in memory" note on `delivery` in
docs/design.md §3. No new dependency: the cache is a bounded `HashMap` behind a `Mutex`.

## Context

Every viewer of an HLS recording fetches the master playlist and a rung playlist, and each fetch
asked Postgres where the ladder lives (`renditions`) and read the playlist object from the
bucket. The answers are the same for every viewer and change only when a ladder is rebuilt, so
200 viewers meant 400+ identical database queries and bucket reads per cycle, on the path
docs/design.md §11 gives an API p95 of 100 ms.

## Decision

1. `DeliveryService` remembers, for 30 s each, **where a recording's ladder lives** (keyed by
   *workspace and recording*, so a lookup under another workspace never sees it) and **what a
   stored playlist says** (keyed by its storage key, which begins with the workspace).
2. **Only what was found is remembered.** A recording with no ladder yet is asked about again on
   every request, so the moment the ladder exists it is served.
3. **Nothing signed is cached.** Tokens and presigned URLs are made per request from the cached
   text; ADR-0033's expiry and the token check are untouched. Access decisions (link,
   visibility, viewer) happen before delivery and are not cached either.
4. The cache is bounded (4096 entries per map): when full, what expired goes first, then
   everything. It uses the injected `Clock`, so tests control time.
5. The staleness window is 30 s: a ladder rebuilt by a retry can be served from the old
   playlists for at most that long, and the old segments still exist until replaced.

## Consequences

- Each API process holds its own cache; there is nothing to invalidate across processes, which is
  why the lifetime is short rather than event-driven. `RenditionReady` (§7) can invalidate on
  purpose later if 30 s ever matters.
- The load test (`just load-playback`) is the measure: k6 from its Docker image, 200 viewers each
  with its own `X-Forwarded-For` address, the production per-address limit in force, and signed
  bucket URLs fetched from MinIO with the original Host header. Results in PROGRESS.md.
