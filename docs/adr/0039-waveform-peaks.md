# ADR-0039: Waveform peaks for the editor

## Status

Accepted — 2026-10-26 (Day 92, M11).

## Context

The editor's timeline (Day 93) shows the recording's sound as a waveform, so trims and cuts can be
placed at pauses. Day 92's Check is that the peaks render in under 200 ms. Nothing in
docs/design.md fixes the format, where the peaks live, or when they are made.

## Decision

1. **A `GenerateWaveform` job**, queued by `ProcessTake` next to `GenerateSprite` (one at a time,
   like the other derived jobs). It decodes the MP4's audio with FFmpeg to raw 8 kHz mono 16-bit
   PCM in the scratch directory and folds it, in `media`'s domain code, into peaks.
2. **Always from the original.** The editor works in the original's time (ADR-0036, ADR-0038), so
   the peaks come from the original MP4 (`mp4/default.mp4` under the recording's own base) and are
   stored as `img/waveform.json` there. An edit never replaces them: the version swap deletes only
   the ladder, sprite and preview rows.
3. **The format** (`version` 1): `{version, duration_ms, bucket_ms, has_audio, peaks}`. A peak is
   the loudest sample of a bucket, 0 (silence) to 255 (full scale), linear. Buckets are 50 ms, or
   longer when that would make more than 20,000 peaks, so the JSON stays under ~80 KB whatever the
   length. There are exactly `ceil(duration_ms / bucket_ms)` peaks: the timeline lines up with the
   recording's length. A recording without an audio track has `has_audio: false` and no peaks.
4. **A new rendition kind `waveform`** (variant `peaks`), in its own migration.
5. **`GET /api/v1/recordings/{id}/waveform`**, owner or admin (the edit permission): a 15-minute
   signed URL of the JSON, like every other stored file. While the peaks don't exist it answers
   `409`, and queues the job for a recording processed before it existed (the same lazy request
   as the HLS ladder's on first view, Day 80).
6. **Rendering cost is bounded by pixels.** The web's `drawWaveform` first folds the peaks to the
   canvas width (each column keeps its loudest) and draws one bar per column, scaled so the loudest
   reaches the edges. "Peaks render in under 200 ms" is measured from the parsed JSON to the
   drawn canvas, for the largest waveform (20,000 peaks).

## Consequences

- Every processed recording gets a few seconds of extra worker time (an audio decode).
- Peaks are linear, not in decibels: quiet speech draws small next to loud parts. The renderer
  scales to the loudest peak, which is enough for finding pauses; a log scale can come later.
- The Day 92 timing test draws on a stand-in context in the unit tests, so it measures the folding
  and the number of draw calls, not the browser's rasterising. Day 93's editor page is where the
  waveform is drawn on a real canvas in Playwright.
