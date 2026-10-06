# ADR-0031: Resume position in the browser; scrub sprite served through the API

## Status

Accepted — 2026-10-06 (Day 84, M10). Fills in docs/design.md §4.6 "resume position, hover sprite,
shortcuts". No new dependency.

## Decision

1. **Resume position is stored in the viewer's browser** (`localStorage`, key per link slug), not
   on the server. Watching needs no account, so there is no identity to key a server row by, and
   §10's view heartbeat (V1, Day 122+) is for analytics, not for the viewer's own convenience.
   Positions under 3 s are not kept; a position within 5 s of the end is forgotten, so a finished
   video starts over. Nothing is saved before the stored position has been applied, and never for
   the original-file preview (it seeks poorly). Storage failures are ignored.
2. **`GET /s/{slug}/sprite.vtt`** returns the stored cue file with each cue's sheet replaced by a
   signed URL (15 minutes) and the `#xywh=` rectangle kept, exactly the technique of the HLS
   playlists (ADR-0030): same access rule as playback, a cue naming anything but a plain file is
   refused, `Viewer` row in the tenant-isolation table. `/playback` returns `sprite_url` once the
   sprite exists.
3. **Scrub bar.** A bar under the video shows progress and, on hover, the thumbnail for that time
   (binary search over the cues) and the time. The native controls stay.
4. **Shortcuts added:** J/L ±10 s, Home/End, 0–9 jump to tenths, `<`/`>` speed, ↑/↓ volume
   (existing: ←/→, space/K, F, M).

## Consequences

- A different browser or a cleared profile starts from the beginning; a signed-in viewer's
  cross-device resume would need a server table (not planned).
