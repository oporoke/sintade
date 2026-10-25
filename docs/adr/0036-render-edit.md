# ADR-0036: Saving and rendering an edit

## Status

Accepted — 2026-10-08 (Day 90, M11). Builds on ADR-0035 (the EDL). Implements docs/design.md §9
`POST /recordings/{id}/edits` and the `RenderEdit` job (§10). §4's key and §7 are amended by
[ADR-0038](0038-edit-versioning.md) (Day 91): the MP4 is at `edits/{version}/mp4/default.mp4`, and
the edit is made the version viewers get once all of it is stored.

## Decision

1. `POST /recordings/{id}/edits` takes `{"ranges": [[start_ms, end_ms], ...]}`, validates it as an
   `Edl` against the **length recorded on the recording's default MP4** (`renditions.meta
   .duration_ms`, what processing measured), saves it as the next version and queues `RenderEdit`,
   in one transaction under a row lock on the recording, so two edits made together get two
   versions. Answers `202` with the version and the kept length.
2. Only the owner or a workspace admin may edit (like chapters); another workspace's recording is
   `404`. A recording without an MP4 yet is `409`. Bad ranges, and ranges that keep everything
   (nothing to render), are `422`.
3. `RenderEdit` cuts the **default MP4**, not the original: it is already a clean, seekable H.264
   file at the plan's resolution. Each range is `trim`med (video) and `atrim`med (audio, if there
   is any), restarted at zero and `concat`enated; the result is re-encoded to H.264/AAC in a
   fast-start MP4 with the same settings as processing. A cut on a source frame boundary is not
   guaranteed, so the video is re-encoded rather than stream-copied.
4. The output is stored at `edits/{version}/default.mp4` (§8 layout) and the edit row records
   `status` (`queued` → `rendering` → `rendered`, or `failed`), `output_key`, the measured
   `output_duration_ms`, size and time.
5. **The rendered length must equal the kept ranges** within 120 ms (a frame or two and an audio
   packet); anything else is a failed render, never a stored one.
6. A failed attempt is retried with the queue's backoff; only the **last** attempt marks the edit
   `failed`. A duplicate job on a rendered edit does nothing. Renders run one at a time per worker.
7. **Nothing a viewer sees changes yet.** The edit exists beside the original; making it the
   recording's playable version (new renditions, the HLS rebuild, `ready → processing → ready`,
   keeping the old version until the new one succeeds) is Day 91.

## Consequences

- An edit of an edit starts from the same default MP4 and EDL ranges in original time; the
  version a viewer watches and the version the editor shows are separate questions for Day 91.
- Rendering the whole recording on every edit is simple and correct; a smart copy of untouched
  GOPs is a later optimisation if renders prove slow.
