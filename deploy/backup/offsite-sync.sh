#!/usr/bin/env bash
# Mirrors the local backups and the media bucket to the off-site S3-compatible store (run every
# few minutes from cron or a systemd timer; the WAL archive is what bounds the RPO).
#   OFFSITE_ALIAS  an `mc` alias for the off-site store (mc alias set …)
#   BACKUP_ROOT    local backup directory (default /backup)
#   MEDIA_ALIAS    an `mc` alias for the production media store, and MEDIA_BUCKET its bucket
# The off-site buckets must have versioning on (docs/design.md §11: storage versioning 7 days):
# a mirror that deletes something deleted by mistake must still be able to bring it back.
set -euo pipefail
alias="${OFFSITE_ALIAS:?set OFFSITE_ALIAS}"
root="${BACKUP_ROOT:-/backup}"

# Backups: --overwrite replaces what changed; no --remove, so pruning on the primary doesn't
# delete the off-site copy. The off-site lifecycle rule expires objects after the retention.
mc mirror --overwrite "$root" "$alias/${OFFSITE_BACKUP_BUCKET:-sintade-backups}"

if [ -n "${MEDIA_ALIAS:-}" ]; then
  mc mirror --overwrite --remove "${MEDIA_ALIAS}/${MEDIA_BUCKET:?set MEDIA_BUCKET}" \
    "$alias/${OFFSITE_MEDIA_BUCKET:-sintade-media}"
fi
