-- Owned by `catalog` (docs/design.md §5, §8). `state` is the Media lifecycle (§4); catalog
-- holds the row, media drives the state from M5 on.
CREATE TYPE recording_state AS ENUM
  ('recording', 'uploading', 'processing', 'ready', 'failed', 'abandoned', 'trashed');

CREATE TABLE recordings (
    id            uuid PRIMARY KEY,
    workspace_id  uuid NOT NULL REFERENCES workspaces(id),
    owner_id      uuid NOT NULL REFERENCES users(id),
    folder_id     uuid,
    title         text NOT NULL,
    description   text,
    state         recording_state NOT NULL DEFAULT 'recording',
    duration_ms   integer,
    width         integer,
    height        integer,
    size_bytes    bigint,
    current_take  uuid,
    trashed_at    timestamptz,
    created_at    timestamptz NOT NULL DEFAULT now(),
    updated_at    timestamptz NOT NULL DEFAULT now(),
    search_tsv    tsvector GENERATED ALWAYS AS
                  (to_tsvector('simple', coalesce(title, '') || ' ' || coalesce(description, ''))) STORED
);

-- A new, empty table, so plain (non-concurrent) index builds are fine here.
CREATE INDEX recordings_ws_created ON recordings (workspace_id, created_at DESC) WHERE trashed_at IS NULL;
CREATE INDEX recordings_search ON recordings USING gin (search_tsv);
