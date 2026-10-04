-- `SweepStaleUploads` (Day 41) marks takes whose upload went idle, so each run moves on to new
-- ones instead of re-reading the same stale takes. Owned by `ingest`.
ALTER TABLE takes ADD COLUMN abandoned_at timestamptz;
