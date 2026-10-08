-- Owned by `media` (docs/design.md §8 `edits`, §4.7 editing). One row per saved edit of a
-- recording: the EDL is the list of kept `[start_ms, end_ms]` ranges of the original, validated
-- against `source_duration_ms` (the duration it was made for) before it is stored. Versions count
-- up from 1 per recording; the original take is never touched (non-destructive).
CREATE TABLE edits (
    id                 uuid PRIMARY KEY,
    workspace_id       uuid NOT NULL REFERENCES workspaces(id),
    recording_id       uuid NOT NULL REFERENCES recordings(id) ON DELETE CASCADE,
    version            integer NOT NULL CHECK (version >= 1),
    edl                jsonb NOT NULL CHECK (jsonb_typeof(edl) = 'array' AND jsonb_array_length(edl) >= 1),
    source_duration_ms integer NOT NULL CHECK (source_duration_ms > 0),
    created_by         uuid NOT NULL REFERENCES users(id),
    created_at         timestamptz NOT NULL DEFAULT now(),
    UNIQUE (recording_id, version)
);

-- A new, empty table, so a plain (non-concurrent) index build is fine here.
CREATE INDEX edits_recording ON edits (workspace_id, recording_id, version DESC);
