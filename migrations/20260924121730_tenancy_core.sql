-- Owned by the (not-yet-existing) tenancy module -- see ADR-0004: identity writes to these
-- tables directly until tenancy is extracted as its own crate on Day 18.
CREATE TYPE member_role AS ENUM ('owner', 'admin', 'member', 'viewer');

CREATE TABLE workspaces (
    id          uuid PRIMARY KEY,
    name        text NOT NULL,
    is_personal boolean NOT NULL DEFAULT false,
    created_at  timestamptz NOT NULL DEFAULT now()
);

CREATE TABLE memberships (
    workspace_id uuid REFERENCES workspaces(id) ON DELETE CASCADE,
    user_id      uuid REFERENCES users(id) ON DELETE CASCADE,
    role         member_role NOT NULL,
    PRIMARY KEY (workspace_id, user_id)
);
