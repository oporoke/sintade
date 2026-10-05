# Runbook: disk full

Find out which disk, then what grew.

| Where | Normal size | Grows when | Fix |
| --- | --- | --- | --- |
| Worker scratch (`WORKER_SCRATCH_DIR`) | ~2× the largest take being processed × concurrent transcodes | A job crashed and left its directory; very long takes | Delete directories older than a few hours (`find $WORKER_SCRATCH_DIR -mindepth 1 -maxdepth 1 -mmin +360 -exec rm -rf {} +`); the worker also sweeps them at start |
| Postgres data / WAL | database size | **WAL archiving is failing**, so Postgres keeps every segment | `backup.md`: fix archiving first; never delete files in `pg_wal` by hand |
| `/backup` | ~15× a compressed base backup + WAL | Pruning stopped (base backups failing) | Run `base-backup.sh` (prunes); check off-site sync |
| MinIO / object storage | grows with recordings | Chunks not purged (sweep not running), `PurgeRecording` not running | `SELECT * FROM jobs WHERE kind IN ('SweepStaleUploads','PurgeRecording') ORDER BY run_at DESC LIMIT 5;` Both run hourly. MinIO answers `507` when full |
| Logs | — | Debug logging left on | Rotate; set `RUST_LOG=info` |

After freeing space, retry what failed: stuck recordings (`worker-stuck.md`) and uploads
(`uploads-failing.md`) recover by themselves or with the retry button.
