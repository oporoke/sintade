-- Owned by `media`. Edit versioning (ADR-0038): a rendered edit is `applied` once its renditions
-- replace the recording's, or `superseded` when a newer edit of the recording was applied first.
ALTER TABLE edits
    DROP CONSTRAINT edits_status_check,
    ADD CONSTRAINT edits_status_check
        CHECK (status IN ('queued', 'rendering', 'rendered', 'applied', 'superseded', 'failed')),
    DROP CONSTRAINT edits_rendered_has_output,
    ADD CONSTRAINT edits_rendered_has_output
        CHECK (status NOT IN ('rendered', 'applied')
               OR (output_key IS NOT NULL AND output_duration_ms IS NOT NULL AND rendered_at IS NOT NULL)),
    ADD COLUMN applied_at timestamptz,
    ADD CONSTRAINT edits_applied_has_time CHECK (status <> 'applied' OR applied_at IS NOT NULL);
