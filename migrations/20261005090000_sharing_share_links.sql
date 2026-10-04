-- Owned by `sharing` (docs/design.md §5, §8). `workspace_id` is added to the §8 DDL (denormalised
-- from the recording) so every query filters by it (CLAUDE.md rule 5). Password and invites are
-- V1: `password_hash` stays NULL for now.
CREATE TYPE visibility AS ENUM ('private', 'workspace', 'link', 'public');

CREATE TABLE share_links (
    id             uuid PRIMARY KEY,
    workspace_id   uuid NOT NULL REFERENCES workspaces(id),
    recording_id   uuid NOT NULL REFERENCES recordings(id) ON DELETE CASCADE,
    slug           text NOT NULL UNIQUE,
    visibility     visibility NOT NULL DEFAULT 'link',
    password_hash  text,
    allow_download boolean NOT NULL DEFAULT false,
    expires_at     timestamptz,
    revoked_at     timestamptz,
    created_at     timestamptz NOT NULL DEFAULT now(),
    CONSTRAINT share_links_slug_shape CHECK (slug ~ '^[0-9A-Za-z]{12}$')
);

-- A new, empty table, so a plain (non-concurrent) index build is fine here.
CREATE INDEX share_links_recording ON share_links (workspace_id, recording_id);
