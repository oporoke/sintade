-- The concatenated original (`source.webm` / `source.mp4`) becomes a rendition so the watch page
-- can play it while the MP4 is still being made (docs/design.md §10 Watch: "serve source.webm
-- directly to Chrome/Firefox while the MP4 builds"). A separate migration: a new enum value can't
-- be used in the transaction that adds it.
ALTER TYPE rendition_kind ADD VALUE IF NOT EXISTS 'source';
