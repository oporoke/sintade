#!/usr/bin/env bash
# Postgres `archive_command`: archive_command = '/backup/bin/archive-wal.sh %p %f'
# Copies one finished WAL segment into $WAL_DIR. Never overwrites a segment with different
# content (that would mean two clusters writing to one archive) and is safe to repeat.
set -euo pipefail
src="$1"
name="$2"
dir="${WAL_DIR:-/backup/wal}"
mkdir -p "$dir"
dest="$dir/$name"
if [ -e "$dest" ]; then
  cmp -s "$src" "$dest" && exit 0
  echo "archive-wal: $name already archived with different content" >&2
  exit 1
fi
tmp="$dest.part.$$"
cp "$src" "$tmp"
sync "$tmp" 2>/dev/null || true
mv "$tmp" "$dest"
