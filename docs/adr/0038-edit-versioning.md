# ADR-0038: Edit versioning — an edit replaces what viewers get only once it is complete

## Status

Accepted — 2026-10-25 (Day 91, M11). Builds on ADR-0035 (the EDL) and ADR-0036 (saving and
rendering an edit), and **amends ADR-0036 §4**: the rendered MP4 moves from
`edits/{version}/default.mp4` to `edits/{version}/mp4/default.mp4`. Implements the
`Ready → Processing: edit applied` transition of docs/design.md §4.

## Context

Day 90 renders an edit's MP4 beside the original, but viewers still get the original. Day 91 must
make the edit the version viewers get (new renditions, the HLS ladder rebuilt,
`ready → processing → ready`) without a viewer ever seeing a half-made version, and keep the old
version until the new one has succeeded.

Playback already reads every object key from the `renditions` rows (MP4, poster, ladder prefix,
sprite VTT), never from a fixed path. Only the jobs that *make* the ladder and the sprite used
fixed paths.

## Decision

1. **One base per version.** Everything a version plays from sits under one base: the original's
   is `ws/{w}/rec/{r}`, an edit's `ws/{w}/rec/{r}/edits/{version}`. Under a base: `mp4/default.mp4`,
   `hls/…`, `img/poster.jpg`, `img/sprite_{n}.jpg`, `img/sprite.vtt`, `img/preview.webp` — the same
   layout as the original's (docs/design.md §8).
2. **`RenderEdit` makes the whole version first.** It moves the recording `ready → processing`,
   renders the MP4 from the **original's** MP4 (ranges are in original time; ADR-0036), and then
   from that MP4 the poster, the sprite and preview, and the HLS ladder **if the recording has
   one** (a ladder is built after the first view, Day 80; without one, the next view asks for one
   cut from the new MP4). A retry reuses an MP4 an earlier attempt stored.
3. **The swap is one transaction**, taken under a per-recording advisory lock
   (`media.versions:{recording}`): the old ladder, sprite and preview rows are deleted (the new
   version may have fewer rungs or sheets), the version's rows are written in their place (MP4,
   poster, sprite, preview, ladder; the original's `source` row stays), the edit becomes
   `applied`, the recording takes the new version's length and size, and `EditApplied` goes into
   the outbox. Viewers get the old version or the new one, never a mix.
4. **Failure leaves the old version.** A failed attempt is retried with the queue's backoff; the
   recording stays `processing` and keeps playing the old version meanwhile. On the last attempt
   the edit becomes `failed`, what it stored under its base is deleted, and the recording goes
   back to `ready` with the renditions and measurements it had.
5. **The newest applied edit wins.** An edit that starts, or finishes, after a newer edit of the
   recording was applied becomes `superseded` and is never shown. The recording goes back to
   `ready` only when no other edit of it is queued, rendering or rendered-but-not-applied.
6. **`BuildHls` and `GenerateSprite` follow the current version.** They read the MP4 viewers get
   from its rendition row, write beside it, and, under the same lock, record their rows only if
   that MP4 is still current; otherwise they report `Superseded` and record nothing.
7. **Further edits use original time.** Submitting validates ranges against the original's length:
   the first edit's `source_duration_ms` once an edit exists, the MP4's measured length before.
8. **`EditApplied`, not `RecordingReady`.** The "ready" email is not sent again for an edit; live
   status wakes on `EditApplied`.
9. `edits.status` gains `applied` and `superseded`; `applied_at` records the swap.

## Consequences

- Viewers keep watching the old version while an edit renders, including on `processing`
  (playback serves the MP4 a `processing` recording has).
- Delivery remembers a ladder's prefix for 30 s (ADR-0034), so a viewer may get the old ladder for
  up to 30 s after a swap. The old version's files are kept, so that still plays.
- Superseded and replaced versions' files stay in storage until the recording is purged; deleting
  them after a grace period is a later step (Deferred in `docs/plan/PROGRESS.md`). Storage use
  for billing doesn't count edit versions yet.
- The status stream does not see the `ready → processing` step (no event marks it); the owner, who
  submitted the edit, knows. It sees the return to `ready` through `EditApplied`.
