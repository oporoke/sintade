#!/usr/bin/env bash
# Prepares an EMPTY data directory to recover to a point in time. Run it inside a Postgres
# container (or host) whose PGDATA is empty, then start Postgres: it replays WAL from the archive
# up to the target and promotes itself.
#   restore-postgres.sh [<UTC time, e.g. "2026-10-05 12:30:00+00"> | latest] [<base backup name>]
# Env: BASE_DIR, WAL_DIR (as for the backup scripts), PGDATA.
set -euo pipefail
target="${1:-latest}"
base_dir="${BASE_DIR:-/backup/base}"
wal_dir="${WAL_DIR:-/backup/wal}"
data="${PGDATA:?set PGDATA}"

if [ -n "$(ls -A "$data" 2>/dev/null)" ]; then
  echo "restore-postgres: $data is not empty; refusing to overwrite a data directory" >&2
  exit 1
fi

# The newest base backup that is not newer than the target (any, for "latest").
backup="${2:-}"
if [ -z "$backup" ]; then
  for candidate in $(ls -1 "$base_dir" | sort -r); do
    if [ "$target" = latest ]; then backup="$candidate"; break; fi
    # Directory names are UTC timestamps (…Z): compare them with the target as UTC.
    t="$(date -u -d "$target" +%Y%m%dT%H%M%SZ)"
    if [ "$candidate" \< "$t" ] || [ "$candidate" = "$t" ]; then backup="$candidate"; break; fi
  done
fi
[ -n "$backup" ] || { echo "restore-postgres: no base backup before $target" >&2; exit 1; }

echo "restoring base backup $backup"
( cd "$base_dir/$backup" && sha256sum -c <(sed -n 's/^sha256=\(.*\)$/\1  base.tar.gz/p' MANIFEST) )
tar -xzf "$base_dir/$backup/base.tar.gz" -C "$data"
chmod 700 "$data"

{
  echo "restore_command = 'cp $wal_dir/%f %p'"
  if [ "$target" != latest ]; then
    echo "recovery_target_time = '$target'"
    echo "recovery_target_inclusive = true"
  fi
  echo "recovery_target_action = 'promote'"
} >> "$data/postgresql.auto.conf"
touch "$data/recovery.signal"
echo "ready: start Postgres to replay WAL up to ${target}"
