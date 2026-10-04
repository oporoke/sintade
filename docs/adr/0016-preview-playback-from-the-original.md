# ADR-0016: Watch the original while the MP4 is made

## Status

Accepted — 2026-10-05 (Day 59, M6).

## Context

US-20 wants "my link is ready right after I stop", and §10 Watch says the watch page "can serve
`source.webm` directly to Chrome/Firefox while the MP4 builds". Day 50 put a 30-minute take's MP4
at 10 minutes, so the MP4 alone cannot meet "stop-to-playable ≤ 5 s" for anything but short takes.

## Decision

1. `ProcessTake` records the stored original as a `source` rendition (new `rendition_kind`
   value) as soon as it is assembled and stored, before the transcode. The MP4 rendition still
   marks the recording `ready`.
2. While a recording is `processing`, `GET /s/{slug}/playback` returns the original with
   `kind: "preview"` and its `content_type` (`video/webm` for Chrome/Firefox recordings,
   `video/mp4` for Safari's); once an MP4 exists it returns `kind: "mp4"`. A `failed` recording's
   original is never offered, and `GET /s/{slug}/download` stays MP4-only.
3. The watch page asks the browser (`canPlayType`) before using a preview, so a Safari viewer of a
   WebM keeps the "still being processed" notice instead of a broken player. It checks the
   recording once a second while it is `processing`, and swaps the player to the MP4 itself: at
   once if nobody is watching, otherwise when the viewer pauses (position kept).
4. A preview is not indexed: MediaRecorder WebM has no cues or duration, so seeking is limited
   and the page says so. Fixing the container (`ffmpeg -c copy` remux of the source) is a possible
   later improvement; it would delay the preview by a pass over the file.

## Consequences

- Stop → playable is about the time to upload the last chunk, concatenate and probe the source
  (1.3–2.5 s on a 6 s take locally), independent of the transcode.
- The signed preview URL points at the cold-tier source object (§11 Storage: `source.webm`
  moves to cold storage after 30 days; previews only happen in the first minutes).
