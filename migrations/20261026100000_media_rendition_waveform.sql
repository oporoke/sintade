-- Owned by `media`. The waveform peaks (`img/waveform.json`) the editor's timeline draws
-- (ADR-0039). A separate migration: a new enum value can't be used in the transaction that adds
-- it.
ALTER TYPE rendition_kind ADD VALUE IF NOT EXISTS 'waveform';
