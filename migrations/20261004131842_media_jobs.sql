-- Owned by `media` (docs/design.md §5). One row per take to process: the chunk manifest that
-- `TakeFinalized` carried (ADR-0012), so processing (and a manual retry) never reads ingest's
-- tables, and a replayed event is a no-op (the primary key).
CREATE TYPE media_job_state AS ENUM ('queued', 'running', 'done', 'failed');

CREATE TABLE media_jobs (
    take_id       uuid PRIMARY KEY REFERENCES takes(id) ON DELETE CASCADE,
    workspace_id  uuid NOT NULL REFERENCES workspaces(id),
    recording_id  uuid NOT NULL REFERENCES recordings(id) ON DELETE CASCADE,
    mime_type     text NOT NULL,
    duration_ms   integer NOT NULL CHECK (duration_ms >= 0),
    chunks        jsonb NOT NULL,                     -- [{idx, key, size_bytes, sha256}]
    state         media_job_state NOT NULL DEFAULT 'queued',
    attempts      integer NOT NULL DEFAULT 0,
    last_error    text,
    created_at    timestamptz NOT NULL DEFAULT now(),
    started_at    timestamptz,
    finished_at   timestamptz
);

-- A new, empty table, so a plain (non-concurrent) index build is fine here.
CREATE INDEX media_jobs_recording ON media_jobs (recording_id);
