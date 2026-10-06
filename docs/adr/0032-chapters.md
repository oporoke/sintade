# ADR-0032: Owner-defined chapters

## Status

Accepted — 2026-10-06 (Day 85, M10). Fills in docs/design.md §4.6 "chapters / markers"; adds the
`chapters` table (not in §8, which only lists AI-made chapters under `intelligence`, V2).

## Decision

1. **`catalog` owns them.** They are recording metadata the owner types, not derived from media.
   Table `chapters (id, workspace_id, recording_id, start_ms, title)`, `UNIQUE (recording_id,
   start_ms)`, every query filtered by `workspace_id`. V2's AI chapters can land in the same table
   (or an `intelligence` one) later without changing the player.
2. **The list is replaced as a whole.** `PUT /recordings/{id}/chapters` takes the full list and
   swaps it in one transaction; `GET` reads it. Both are `Workspace` routes for the owner or an
   admin (as rename), `404` outside the workspace, rows in the tenant-isolation table.
3. **Validation lives in the domain** (`ChapterList`): at most 100, title 1–120 characters with no
   control characters, unique starts, start ≤ the recording's duration when known, sorted on the
   way in. Violations are `422`; nothing is stored.
4. **Viewers get them with the watch data** (`WatchResponse.chapters`), so access follows the
   link exactly and no new public route is needed.
5. **UI.** The owner edits chapters as text, one per line (`1:05 Demo`), in a dialog on the library
   card. The watch page shows them as a table of contents under the video (the current one
   marked) and as ticks on the scrub bar; clicking seeks.

## Consequences

- Chapters are not shifted when an edit (M11) removes ranges from the recording; Day 91's
  versioning has to decide how (`TODO: Verify`).
