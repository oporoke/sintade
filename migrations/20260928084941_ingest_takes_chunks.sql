-- Owned by `ingest` (docs/design.md §5, §8). Both tables carry `workspace_id`, which §8's sketch
-- leaves out, so every query can filter by tenant directly (CLAUDE.md rule 5; ADR-0009).
CREATE TABLE takes (
    id               uuid PRIMARY KEY,
    workspace_id     uuid NOT NULL REFERENCES workspaces(id),
    recording_id     uuid NOT NULL REFERENCES recordings(id) ON DELETE CASCADE,
    mime_type        text NOT NULL,           -- e.g. video/webm;codecs=vp9,opus
    has_system_audio boolean NOT NULL,
    has_mic          boolean NOT NULL,
    has_camera       boolean NOT NULL,
    finalized_at     timestamptz,
    created_at       timestamptz NOT NULL DEFAULT now()
);

-- A new, empty table, so a plain (non-concurrent) index build is fine here.
CREATE INDEX takes_recording ON takes (recording_id);

CREATE TABLE chunks (
    take_id      uuid REFERENCES takes(id) ON DELETE CASCADE,
    workspace_id uuid NOT NULL REFERENCES workspaces(id),
    idx          integer NOT NULL CHECK (idx >= 0),
    size_bytes   integer NOT NULL CHECK (size_bytes > 0),
    sha256       bytea NOT NULL CHECK (octet_length(sha256) = 32),
    received_at  timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (take_id, idx)
);
