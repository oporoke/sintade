# ADR-0030: Adaptive playback through API-served playlists and hls.js

## Status

Accepted — 2026-10-06 (Day 83, M10). Fills in docs/design.md §10 Watch step 4 ("Player picks HLS
or MP4 fallback"). `hls.js` is already named in §2; no other dependency is added. Day 86 hardens
what is decided here (token per manifest).

## Context

Day 79–80 build the ladder under `hls/` after a recording's first view. A master playlist names
each rung relatively and a rung playlist names its init and media segments relatively. The bucket
is private, so a browser can only fetch an object with a presigned URL, and a signature covers one
object: a relative reference from a signed playlist would resolve to an unsigned URL.

## Decision

1. **The API serves the playlists, not the bucket.** `GET /s/{slug}/hls/master.m3u8` returns the
   stored master as is; its relative `360p/index.m3u8` references resolve against the API URL.
   `GET /s/{slug}/hls/{rung}/index.m3u8` reads the rung's playlist from storage and replaces the
   `#EXT-X-MAP` URI and every segment line with a presigned GET (15 minutes, like every other
   grant). Segments themselves still go browser → storage, never through the API (rule 4).
2. **Same access rule as playback.** Both routes resolve the link exactly as `/playback` does:
   hidden is `404`, a login requirement is `401`. They are `Viewer` rows in the tenant-isolation
   table, under the public watch rate limit.
3. **A playlist may only name plain files.** Names with a path, a leading dot or a scheme are
   refused (`404` and an error log), so a stored playlist can never get a signature for another
   recording's object. Playlists are read with a 1 MiB cap.
4. **`GET /s/{slug}/playback` offers the ladder.** `hls_url` is present once the MP4 is the
   grant and a ladder exists; the MP4 `url` is always there as the fallback.
5. **Player.** `VideoPlayer` (`web/.../hls-player.ts`) plays natively on Safari (vendor check:
   Chromium answers `maybe` to `canPlayType` for HLS but cannot pick a rung), through a lazily
   imported hls.js elsewhere, and falls back to the MP4 if neither works or if adaptive playback
   fails for good. A quality change sets `nextLevel`, which switches at the next fragment
   without flushing the buffer. A network error re-reads the playlists once (the segment
   signatures expire after 15 minutes), a media error is recovered once, then the MP4 takes over
   at the same position.

## Consequences

- A viewer costs three small API requests (master, rung, and a re-read after a long pause) on
  top of the watch page's own; they are inside the 120/min watch budget.
- Safari's native player has no quality menu here; it adapts by itself.
- Segment URLs are not bound to a viewer: anyone holding a playlist can share its URLs for 15
  minutes. Day 86 addresses hotlinking.
