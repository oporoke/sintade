# Runbook: API 5xx spike

1. **Recent deploy?** Roll back first, investigate second: re-run the deploy job with the previous
   tag (§19). Migrations are forward-only and compatible, so the old image runs on the new schema.
2. **Database:** `/readyz` is a `SELECT 1`. Connections exhausted → `SELECT state, count(*) FROM pg_stat_activity GROUP BY 1;`;
   slow queries → `SELECT now() - query_start AS age, query FROM pg_stat_activity WHERE state = 'active' ORDER BY age DESC LIMIT 5;`
3. **Rate limiter:** a failing `rate_limit_buckets` table is logged (`rate limiter unavailable`)
   and *does not* cause errors (requests go through); it still points at a database problem.
   A table that grows without bound: old rows are overwritten per key, so size tracks distinct
   callers; prune rows older than a day if it matters.
4. **Logs:** every request carries `x-request-id`; the same id is in the problem+json the client
   saw and in the server log. Never expect tokens, emails or signed URLs in logs (they are not
   logged); if you find one, that is a bug to fix.
5. **Dependencies:** SMTP down does not fail requests (emails are jobs); storage down fails
   presign/playback only.
