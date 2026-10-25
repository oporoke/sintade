-- Owned by `media`. What rendering an edit needs on its row (ADR-0036): the take whose MP4 is cut,
-- where the rendered MP4 is stored, how long it came out, and how far the job got.
ALTER TABLE edits
    ADD COLUMN take_id            uuid REFERENCES takes(id) ON DELETE CASCADE,
    ADD COLUMN status             text NOT NULL DEFAULT 'queued'
        CHECK (status IN ('queued', 'rendering', 'rendered', 'failed')),
    ADD COLUMN output_key         text,
    ADD COLUMN output_duration_ms integer,
    ADD COLUMN output_bytes       bigint,
    ADD COLUMN rendered_at        timestamptz,
    ADD COLUMN failure            text,
    ADD CONSTRAINT edits_rendered_has_output
        CHECK (status <> 'rendered' OR (output_key IS NOT NULL AND output_duration_ms IS NOT NULL AND rendered_at IS NOT NULL));
