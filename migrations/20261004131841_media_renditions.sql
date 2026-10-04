-- Owned by `media` (docs/design.md §5, §8). Each processed take produces renditions (the
-- fast-start MP4, the poster, later HLS, sprites and captions). `workspace_id` is added to §8's
-- sketch so every query filters by tenant directly (CLAUDE.md rule 5), as ADR-0009 did for takes.
CREATE TYPE rendition_kind AS ENUM ('mp4', 'hls', 'thumbnail', 'preview', 'sprite', 'captions', 'audio');

CREATE TABLE renditions (
    id            uuid PRIMARY KEY,
    workspace_id  uuid NOT NULL REFERENCES workspaces(id),
    recording_id  uuid NOT NULL REFERENCES recordings(id) ON DELETE CASCADE,
    take_id       uuid NOT NULL REFERENCES takes(id) ON DELETE CASCADE,
    kind          rendition_kind NOT NULL,
    variant       text NOT NULL DEFAULT 'default',   -- '720p', 'poster', 'en', ...
    storage_key   text NOT NULL,
    size_bytes    bigint,
    meta          jsonb NOT NULL DEFAULT '{}',
    created_at    timestamptz NOT NULL DEFAULT now(),
    UNIQUE (recording_id, take_id, kind, variant)
);
