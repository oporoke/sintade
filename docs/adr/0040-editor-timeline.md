# ADR-0040: The editor works on the original, on one timeline that spans it

## Status

Accepted — 2026-10-26 (Day 93, M11). Builds on ADR-0036 (ranges in the original's time),
ADR-0038 (edit versioning) and ADR-0039 (waveform peaks of the original).

## Context

Day 93 adds the editor page: the recording above a timeline with its waveform and thumbnails.
Its Check is that the timeline matches the video's length. Edit ranges are always in the
original's time (ADR-0036), and the peaks are the original's (ADR-0039). But after an edit is
applied, the recording's MP4 and sprite rows point at the edited version (ADR-0038), so
the playback grant and `GET /s/{slug}/sprite.vtt` would show the editor a shorter video whose
times don't match the ranges it edits. Nothing in docs/design.md says how the editor gets the
original.

## Decision

1. **`GET /api/v1/recordings/{id}/editor`** (owner or admin, the edit permission, like
   `/waveform`): `{video_url, duration_ms, sprite_vtt, expires_in_s}`. The video is the
   **original's** MP4 (`mp4/default.mp4` under the recording's own base, which edits never
   replace), signed for 15 minutes. `duration_ms` is the original's length, the one edit ranges
   are checked against (the first edit's `source_duration_ms` once there is one, the MP4's
   measured length before). `sprite_vtt` is the original's `img/sprite.vtt` with each sheet
   signed, inline, or `null` when the original has no sprite. `409` until the recording has an
   MP4; `no-store`.
2. **The keys come from `media`.** `RenditionReader::original` names the original's MP4 and sprite
   and its length, so the storage layout stays in `media`; `delivery` signs them.
3. **One axis, start to end.** The timeline's left edge is 0 and its right edge is exactly
   `duration_ms`: the playhead, clicks and keys map linearly between the two (`timeline.ts`). The
   thumbnail strip fills the width with as many thumbnails as fit (54 px tall), each covering an
   equal stretch and showing the sprite cue at its middle. The waveform canvas spans the same width
   and draws one bar per pixel column (ADR-0039). While the peaks are being made the page asks
   again every 2 s, up to 30 times.
4. **The page** is `/recordings/:id/edit` (signed in), opened from an **Edit** link on each ready
   recording in the library. Day 93 shows and seeks; trimming, cutting and saving come on Days
   94–96.

## Consequences

- The editor plays a progressive MP4, not the HLS ladder: fine for the owner's own recording,
  and it is what edits are cut from.
- A recording whose sprite was made only after an edit (its original's sprite never existed) has
  no thumbnails in the editor; the timeline still works.
- "Matches the video's length" is checked in Playwright against the `<video>` element's own
  `duration`, within 100 ms.
