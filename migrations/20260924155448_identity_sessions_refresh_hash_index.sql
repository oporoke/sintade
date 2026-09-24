-- no-transaction
-- Refresh rotation (Day 14) looks up a session by refresh_hash on every /auth/refresh call --
-- needs an index, and it must be unique since a hash collision here would let one refresh token
-- match a different session's row.
CREATE UNIQUE INDEX CONCURRENTLY IF NOT EXISTS sessions_refresh_hash_idx ON sessions (refresh_hash);
