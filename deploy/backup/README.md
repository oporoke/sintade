# Backups

What is backed up, how, and where it goes (docs/design.md §11 Reliability; runbooks in
`docs/runbooks/`). The scripts here are the only backup tooling; `restore-drill.sh` proves them.

| Data | Mechanism | Frequency | Retention | Off-site |
| --- | --- | --- | --- | --- |
| Postgres | Continuous WAL archiving (`archive-wal.sh`) + nightly base backup (`base-backup.sh`) = point-in-time recovery | WAL: each segment and at least every 60 s (`archive_timeout`); base: nightly | 14 days | `offsite-sync.sh` every 5 min |
| Media (MinIO) | `mc mirror --remove` of the media bucket to a versioned off-site bucket | every 5 min | versions 7 days | the mirror *is* off-site |
| Secrets | Not backed up here: GitHub environment secrets are the source (§19) | — | — | — |

**RPO ≈ 5 min** (60 s `archive_timeout` + the 5-minute sync); **RTO ≈ minutes** for the
database (drill: restore and replay in seconds for a small database; add the download time of the
base backup) plus the time to copy the media bucket back. §11 targets RPO 5 min / RTO 1 h.

## Postgres settings (production)

```
wal_level = replica
archive_mode = on
archive_command = '/opt/sintade/backup/archive-wal.sh %p %f'
archive_timeout = 60
```

`/backup` (or `BACKUP_ROOT`) holds `wal/` and `base/` and belongs to the `postgres` user, mode
`0700`: a backup contains every user's data. Disk for it: roughly the database size × 1 (one
compressed base backup) × 14 (retention) plus the WAL; alert on free space (`disk-full.md`).

## Schedule (systemd timers or cron on the database host)

```
# nightly base backup, 02:30 UTC
30 2 * * *  postgres  /opt/sintade/backup/base-backup.sh
# off-site mirror, every 5 minutes (mc aliases configured for the postgres user)
*/5 * * * *  postgres  OFFSITE_ALIAS=offsite MEDIA_ALIAS=prod MEDIA_BUCKET=sintade-prod /opt/sintade/backup/offsite-sync.sh
```

The off-site store must be a different provider/region from the primary, with **versioning on**
and a lifecycle rule expiring noncurrent versions after 7 days and backups after 14.
Which provider: `TODO: Verify` (decided with the production hosts, Day 69).

## Monitoring

- `pg_stat_archiver.failed_count` must not grow; `last_archived_time` must be younger than 5 min.
  Alert on both.
- The newest `base/*/MANIFEST` must be younger than 26 h.
- The newest off-site object must be younger than 15 min.
- Run `restore-drill.sh` after any change to these scripts and monthly (CI runs it weekly).
