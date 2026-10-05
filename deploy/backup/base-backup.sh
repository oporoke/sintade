#!/usr/bin/env bash
# Nightly base backup (run inside the Postgres container or any host with pg_basebackup):
#   /backup/bin/base-backup.sh
# Writes $BASE_DIR/<UTC timestamp>/{base.tar.gz,backup_label.txt,MANIFEST}, then prunes base
# backups older than $RETENTION_DAYS (default 14, docs/design.md §11) and the WAL segments no
# kept base backup needs. WAL is archived continuously by archive-wal.sh, so a base backup plus
# the WAL after it restores to any moment (point-in-time recovery).
set -euo pipefail
base_dir="${BASE_DIR:-/backup/base}"
wal_dir="${WAL_DIR:-/backup/wal}"
retention="${RETENTION_DAYS:-14}"
stamp="$(date -u +%Y%m%dT%H%M%SZ)"
out="$base_dir/$stamp"
mkdir -p "$out"

# -X none: the WAL needed comes from the archive, not from the backup stream.
pg_basebackup -D - -Ft -X none -c fast -d "${PGCONNINFO:-}" --label "sintade-$stamp" \
  | gzip -9 > "$out/base.tar.gz.part"
mv "$out/base.tar.gz.part" "$out/base.tar.gz"

# The first WAL file recovery will need is named in backup_label inside the archive.
tar -xzOf "$out/base.tar.gz" backup_label > "$out/backup_label.txt"
start_wal="$(sed -n 's/^START WAL LOCATION: .*(file \(.*\))$/\1/p' "$out/backup_label.txt")"
{
  echo "created_utc=$stamp"
  echo "start_wal=$start_wal"
  echo "size_bytes=$(stat -c %s "$out/base.tar.gz")"
  echo "sha256=$(sha256sum "$out/base.tar.gz" | cut -d' ' -f1)"
} > "$out/MANIFEST"
echo "base backup $stamp: $(cat "$out/MANIFEST" | tr '\n' ' ')"

# Prune: drop base backups past retention, always keep the newest one, then WAL older than the
# oldest remaining base backup's start segment.
cutoff="$(date -u -d "-${retention} days" +%Y%m%dT%H%M%SZ)"
newest="$(ls -1 "$base_dir" | sort | tail -n 1)"
for dir in $(ls -1 "$base_dir" | sort); do
  if [ "$dir" \< "$cutoff" ] && [ "$dir" != "$newest" ]; then
    rm -rf "${base_dir:?}/$dir"
  fi
done
oldest="$(ls -1 "$base_dir" | sort | head -n 1)"
oldest_wal="$(sed -n 's/^start_wal=//p' "$base_dir/$oldest/MANIFEST")"
if [ -n "$oldest_wal" ]; then
  for file in "$wal_dir"/*; do
    [ -e "$file" ] || continue
    name="$(basename "$file")"
    case "$name" in
      ????????????????????????) [ "$name" \< "$oldest_wal" ] && rm -f "$file" ;;
      *.history|*.backup) ;;  # tiny, and a timeline history is needed to restore across a promotion
    esac
  done
fi
