# Runbook: backups

Design and numbers: `deploy/backup/README.md`. This is what to do with them.

## Is it healthy? (daily glance, or when an alert fires)

```sql
-- on the database host
SELECT archived_count, failed_count, last_archived_wal, last_archived_time, last_failed_time
FROM pg_stat_archiver;
```

- `last_archived_time` older than 5 minutes while the database is busy → archiving is stuck. Check
  disk space on `/backup`, the `archive-wal.sh` error in the Postgres log, permissions.
  **Postgres keeps WAL it could not archive**: a long stall fills the disk (`disk-full.md`).
- `failed_count` rising → same.
- Newest base backup: `ls -1 /backup/base | sort | tail -1` should be today's or yesterday's.
  A missing night: run `/opt/sintade/backup/base-backup.sh` by hand and find out why the timer didn't.
- Off-site: `mc ls --recursive offsite/sintade-backups | sort | tail` shows objects from the last
  15 minutes; `mc version info offsite/sintade-backups` says versioning is enabled.

## A backup failed

| Symptom | Cause | Fix |
| --- | --- | --- |
| `archive-wal: … already archived with different content` | Two clusters archiving to one directory (a restored copy started with archiving on) | Stop the wrong one. Never delete the segment. If the wrong one was a restore test, point it at its own `WAL_DIR` |
| `base-backup.sh` fails to connect | Postgres down, or `PGCONNINFO` wrong | Fix the connection; run again |
| Off-site sync errors | Credentials, network, bucket quota | `mc ls offsite` interactively; rotate keys (§19) |
| Backups directory full | Retention not pruning (base backups failing) or WAL piling up | Run `base-backup.sh` (it prunes), then see `disk-full.md` |

## Prove it still works

`deploy/backup/restore-drill.sh` (also `just restore-drill`): builds a database from the real
migrations, takes a base backup, wipes data after a recovery point, restores from the *off-site*
copy to the recovery point, and does the same for the media bucket, including undoing a propagated
delete from versions. Run it after touching the backup scripts and monthly. The drill's last
result is in the CI `backup-drill` job and in `docs/runbooks/restore.md` (drill log).
