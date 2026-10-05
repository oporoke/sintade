# Runbook: restore

**When:** data corruption, an accidental mass delete, a lost database host. *Not* for a bad
deploy: roll back the app instead (migrations are forward-only and compatible, §19).

**Decide first:** the recovery point. "Just before the bad thing": the time of the bad query from
the logs, minus a minute. Everything after it is lost, so say it out loud to whoever is asking.

## Restore Postgres to a point in time

1. **Stop writers.** Scale the API and worker to zero (or stop the containers). Writing to the
   old database while you restore makes two histories.
2. **Keep the evidence.** Do not delete the damaged data directory; move it aside
   (`mv /var/lib/postgresql/data /var/lib/postgresql/data.damaged`).
3. **Get the backups** if the host is gone: `mc mirror --overwrite offsite/sintade-backups /backup`.
4. **Prepare an empty data directory** as the `postgres` user:
   ```bash
   mkdir -p $PGDATA && chmod 700 $PGDATA
   BASE_DIR=/backup/base WAL_DIR=/backup/wal PGDATA=$PGDATA \
     /opt/sintade/backup/restore-postgres.sh "2026-10-05 12:30:00+00:00"
   ```
   (`latest` replays everything archived.) It checks the base backup's SHA-256, unpacks it, and
   writes the recovery settings.
5. **Start Postgres.** It replays WAL up to the target and promotes itself; watch the log for
   `recovery stopping before commit of transaction …` and `archive recovery complete`.
   `SELECT pg_is_in_recovery();` returns `f` when it is done.
6. **Check** with a few counts you can reason about (`recordings` per workspace, newest
   `created_at`), and that `_sqlx_migrations` lists the versions the deployed app expects.
7. **Start the API and worker.** Jobs that were mid-flight re-run (jobs are idempotent, §10);
   `SELECT kind, count(*) FROM jobs WHERE done_at IS NULL AND dead_at IS NULL GROUP BY 1;`.
8. **Media can be ahead of the database** (objects uploaded after the recovery point exist, rows
   do not): harmless; `SweepStaleUploads` and `PurgeRecording` never touch unreferenced prefixes,
   and the uploader re-sends chunks the server doesn't know about. Do not delete them by hand.
9. **Write the post-mortem** (`docs/incidents/`), including how long it took.

## Restore the media bucket

```bash
# everything
mc mirror --overwrite offsite/sintade-media prod/sintade-prod
# one object deleted by mistake (versioned off-site bucket)
mc ls --versions offsite/sintade-media/ws/<ws>/rec/<rec>/
mc undo offsite/sintade-media/ws/<ws>/rec/<rec>/mp4/default.mp4
```
If the mirror ran after the bad delete (it uses `--remove`), the current versions are delete
markers: `mc undo --recursive offsite/sintade-media/<prefix>/` brings the previous versions back;
then mirror forward. Do this within the 7-day version retention.

## Drill log

| Date | Scenario | Result |
| --- | --- | --- |
| 2026-10-05 | `restore-drill.sh` on a laptop: 40 + 25 recordings before the recovery point restored, 15 after it correctly absent, table wiped by the "disaster" recovered; media bucket (5 objects) wiped and restored byte for byte; a propagated delete undone from versions | **Passed**, 43 s end to end, Postgres restore + replay 4 s (small database) |
| *staging* | Restore of staging from the off-site copy | **Not yet done: no staging environment exists** (M1 carry-over); do it the day the staging host exists and record it here |
