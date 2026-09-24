-- no-transaction
-- Day 15's verify-email and password-reset flows both look up a row by token_hash on every
-- call -- needs an index, and it must be unique for the same reason as sessions.refresh_hash
-- (Day 14): a hash collision here would let one token match a different user's row.
CREATE UNIQUE INDEX CONCURRENTLY IF NOT EXISTS email_tokens_hash_idx ON email_tokens (token_hash);
