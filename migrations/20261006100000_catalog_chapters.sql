-- Owned by `catalog` (docs/design.md §4.6 chapters, §8). Owner-defined chapters of a recording:
-- the whole list is replaced in one transaction, so a start time is unique per recording.
CREATE TABLE chapters (
    id            uuid PRIMARY KEY,
    workspace_id  uuid NOT NULL REFERENCES workspaces(id),
    recording_id  uuid NOT NULL REFERENCES recordings(id) ON DELETE CASCADE,
    start_ms      integer NOT NULL CHECK (start_ms >= 0),
    title         text NOT NULL CHECK (char_length(title) BETWEEN 1 AND 120),
    created_at    timestamptz NOT NULL DEFAULT now(),
    UNIQUE (recording_id, start_ms)
);

-- A new, empty table, so a plain (non-concurrent) index build is fine here.
CREATE INDEX chapters_recording ON chapters (workspace_id, recording_id, start_ms);
