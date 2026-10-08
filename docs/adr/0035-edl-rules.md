# ADR-0035: The edit decision list

## Status

Accepted — 2026-10-08 (Day 89, M11). Fills in docs/design.md §8 `edits` ("EDL = list of kept
`[start_ms, end_ms]` ranges"). The test dependency `proptest` is the one §18 names for EDL maths.

## Decision

1. An edit is **the ranges kept** of the original, never the ranges cut. It is stored as JSON
   `[[start_ms, end_ms], ...]`, start inclusive, end exclusive, in milliseconds of the original.
   The original take is never changed; every edit is a new version (non-destructive).
2. `media::Edl::new` is the only constructor. It accepts ranges in any order and returns them
   sorted. **Overlap is an error; touching ranges are joined** (a cut of zero length is no cut).
3. Limits: a range is at least `MIN_RANGE_MS` = 100 ms, an edit has 1 to `MAX_RANGES` = 100
   ranges, and no range ends after the recording's duration. The duration is stored with the edit
   (`source_duration_ms`), so an edit is always checkable against the recording it was made for.
4. `edits` is one row per saved version: `UNIQUE (recording_id, version)`, version ≥ 1, the EDL a
   non-empty JSON array (the database refuses anything else; the domain type is the real
   validator). Rows go with the recording (`ON DELETE CASCADE`).
5. Rendering state is not in this table yet: Days 90–91 add what rendering needs in a later
   migration, once the shape of `RenderEdit` is known.

## Consequences

- The editor UI and the render job share one type; mapping source time to edited time (for
  preview and chapters) is added by the day that needs it, on `Edl`.
- A 100-range cap bounds the FFmpeg command and the UI timeline; raising it is one constant.
