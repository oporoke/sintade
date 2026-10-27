# ADR-0041: Trim handles and the in-browser trim preview

## Status

Accepted — 2026-10-27 (Day 94, M11). Builds on ADR-0040 (the editor's timeline in the original's
time).

## Context

Day 94 adds trim handles to the editor's timeline with an instant preview. Its Check is that the
preview starts and ends at the handles. A server render takes seconds to minutes, so the preview
has to happen in the browser, on the original MP4 the editor already plays.

## Decision

1. **Two handles, one kept stretch.** The trim is `{startMs, endMs}` in the original's time,
   from the whole recording by default. Each handle stays inside the recording and on its own
   side of the other, with at least 100 ms between them: the API's shortest range
   (`MIN_RANGE_MS`), so any trim the handles allow can be saved as an edit.
2. **Instant preview of a handle.** Grabbing or moving a handle (drag, or the arrow keys: 100 ms,
   1 s with Shift) pauses the video and shows the frame at the handle. What is trimmed away is
   shaded on the timeline. The handles' grips sit in a row above the timeline, and only a thin line
   that lets clicks through marks them on it, so every point of the timeline, its two ends
   included, can still be clicked to seek (ADR-0040).
3. **The preview plays the original, kept inside the trim.** Preview seeks to the start handle and
   plays. While playing, the video is checked on every animation frame, and the frame that reaches
   the end handle pauses it and puts it exactly at the end handle. `timeupdate` (every ~250 ms) is
   too coarse for that. `requestVideoFrameCallback` isn't used: a screen recording can go seconds
   without a new frame. The native play button follows the same rule: started before the start
   handle, after the end handle or at it, playback jumps to the start handle.
4. **The rule is framework-free** (`trim.ts`, `trim-preview.ts`), so it is tested without a
   browser and reused for the cuts of Day 95.
5. **Nothing is saved yet.** Saving the trim (and Day 95's cuts) as the kept ranges of
   `POST /recordings/{id}/edits` comes with Day 95, whose Check compares the preview with the
   final render.

## Consequences

- The preview stops up to one animation frame (~16 ms) past the end handle and then goes back
  onto it; the e2e test allows 50 ms on the final position.
- No server work: a preview costs nothing and works while an earlier edit is still rendering.
