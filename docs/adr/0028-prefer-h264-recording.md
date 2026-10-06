# ADR-0028: Record H.264 in WebM where the browser can

## Status

Accepted — 2026-10-05 (Day 78, M9). Changes the format preference in docs/design.md §10 Record
(VP9, VP8, then H.264 MP4 for Safari).

## Context

§11 asks for client CPU ≤ 25 % of a 4-core laptop (one core) at 1080p30. Day 78 measured the
test browser's processes (`just profile-capture`) while `MediaRecorder` encoded a 1080p30 canvas
at 5 Mbit/s, in cores on this 8-core machine (software encoders; load from other programs
present):

| Codec | Cores (renderer) |
| --- | --- |
| nothing recorded (the synthetic screen alone) | 0.4 |
| VP9 | 3.0 |
| VP8 | 4.8 |
| H.264 | 1.2 |

VP9 alone is about three times the whole budget. H.264 is the only option that can meet it, and
on real laptops Chrome hands it to the hardware encoder (VideoToolbox, Media Foundation, VA-API).

## Decision

1. `selectMimeType` tries `video/webm;codecs=h264,opus` (video-only: `…codecs=h264`) first, then
   the old order (VP9, VP8, Safari's MP4). Browsers that cannot encode H.264 in WebM — Firefox,
   and open-source Chromium builds such as Playwright's in CI — are unchanged.
2. **The server needs nothing new.** Ingest accepts any `video/webm` codec string; the worker
   probes the file, and H.264 + Opus is not remuxable (the MP4 needs AAC), so it takes the
   existing transcode path, exactly as VP9 did. Verified with a real Chrome recording through the
   worker: `ready`, MP4 and poster present.
3. **Viewer compatibility.** The stored original is played as a preview while the MP4 is made
   (Day 59), and not every browser can play H.264 inside WebM (Firefox, `TODO: Verify`). The
   source rendition's `content_type` now carries the codecs for H.264-in-WebM
   (`media::preview_content_type`), so the watch page's `canPlayType` check says no and the viewer
   waits for the MP4 instead of seeing a broken player. Other recordings keep the bare container.
4. The compositor's canvas is kept to even sizes (H.264 4:2:0 cannot encode odd ones) and is
   created opaque and unsynchronised, and not cleared when the screen covers the frame.

## Consequences

- 1080p30 meets §11 even with a *software* H.264 encoder (net 19–22 % of four cores, with or
  without the camera). 1080p60 does not, in software (net 34–38 %): §11 states no 60 fps target;
  the profile asserts a two-core guardrail and the owner is asked to confirm it.
- A later server optimisation can copy the H.264 video and transcode only the audio, which would
  make processing much faster than today's full re-encode (deferred).
- Firefox viewers of a Chrome H.264 recording get no instant preview until the MP4 is ready.
