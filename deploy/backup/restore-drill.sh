#!/usr/bin/env bash
# The restore drill (docs/runbooks/restore.md): proves, end to end and from scratch, that a lost
# database and a lost media bucket can be brought back from the off-site copies, to a chosen
# moment. Everything runs in throwaway Docker containers; it never touches the dev stack.
#
#   deploy/backup/restore-drill.sh        (needs Docker; takes about two minutes)
#
# 1. Postgres with WAL archiving, the real migrations, some rows.
# 2. A base backup, more rows, a recovery point T, still more rows, then a "disaster" (the
#    recordings table is wiped), all WAL archived.
# 3. Everything is mirrored to a second, versioned MinIO ("off-site") and the primary's backup
#    directory is thrown away.
# 4. A new Postgres is restored from the off-site copy to T: the rows before T are back, the
#    rows after T and the disaster are not. Timed (the RTO).
# 5. The same for the media bucket: objects wiped, restored from the off-site mirror, checksums
#    compared; and a mistaken delete that the mirror propagated is undone from the versions.
set -euo pipefail
cd "$(dirname "$0")/../.."

PG=postgres:16
MINIO=ghcr.io/oporoke/sintade/minio:RELEASE.2025-09-07T16-13-09Z
run="drill$$"
net="$run-net"
work="$(mktemp -d)"
chmod 777 "$work"
mkdir -p "$work/backup"
chmod 777 "$work/backup"
scripts="$PWD/deploy/backup"
started=$(date +%s)

log() { printf '\n== %s\n' "$*"; }
fail() {
  echo "DRILL FAILED: $*" >&2
  docker logs "$run-pg-restore" 2>&1 | tail -25 >&2 || true
  exit 1
}
cleanup() {
  docker rm -f "$run-pg" "$run-pg-restore" "$run-src" "$run-off" >/dev/null 2>&1 || true
  docker volume rm -f "$run-pgdata" "$run-restored" >/dev/null 2>&1 || true
  docker network rm "$net" >/dev/null 2>&1 || true
  rm -rf "$work" 2>/dev/null || docker run --rm -v "$work:/w" "$PG" rm -rf /w/backup /w/restore 2>/dev/null || true
}
trap cleanup EXIT
docker network create "$net" >/dev/null

psql_in() { docker exec -i "$1" psql -qAtX -U postgres -d "${2:-sintade}" -v ON_ERROR_STOP=1; }
mc() { # mc <args…> with aliases src/off preconfigured
  docker run --rm --network "$net" -v "$work:/work" \
    -e MC_HOST_src="http://minio:minio-secret@$run-src:9000" \
    -e MC_HOST_off="http://minio:minio-secret@$run-off:9000" \
    --entrypoint mc "$MINIO" "$@"
}

log "1. Postgres with WAL archiving and the real migrations"
docker run -d --name "$run-pg" --network "$net" -e POSTGRES_PASSWORD=drill \
  -v "$run-pgdata:/var/lib/postgresql/data" -v "$work/backup:/backup" -v "$scripts:/backup/bin:ro" \
  "$PG" postgres -c wal_level=replica -c archive_mode=on \
  -c "archive_command=/backup/bin/archive-wal.sh %p %f" -c archive_timeout=60 >/dev/null
until docker exec "$run-pg" pg_isready -U postgres -q; do sleep 1; done
sleep 2
echo "CREATE DATABASE sintade" | psql_in "$run-pg" postgres >/dev/null
for file in migrations/*.sql; do
  psql_in "$run-pg" < "$file" >/dev/null || fail "migration $file"
done
echo "applied $(ls migrations/*.sql | wc -l) migrations"

seed() { # seed <label> <count>: users + workspaces + recordings
  psql_in "$run-pg" <<SQL
INSERT INTO users (id, email, display_name)
  SELECT gen_random_uuid(), '$1-' || n || '@drill.test', 'Drill $1' FROM generate_series(1, $2) n;
INSERT INTO workspaces (id, name) SELECT gen_random_uuid(), '$1 workspace ' || n FROM generate_series(1, $2) n;
INSERT INTO recordings (id, workspace_id, owner_id, title, state)
  SELECT gen_random_uuid(), w.id, u.id, '$1 recording', 'ready'
  FROM (SELECT id, row_number() OVER () rn FROM workspaces WHERE name LIKE '$1 workspace %') w
  JOIN (SELECT id, row_number() OVER () rn FROM users WHERE email LIKE '$1-%') u USING (rn);
SQL
}
count() { echo "SELECT count(*) FROM recordings WHERE title = '$2 recording'" | psql_in "$1"; }
archive_all() {
  echo "SELECT pg_switch_wal()" | psql_in "$run-pg" >/dev/null
  local want
  want=$(echo "SELECT pg_walfile_name(pg_current_wal_lsn() - 1)" | psql_in "$run-pg")
  for _ in $(seq 1 60); do
    last=$(echo "SELECT coalesce(last_archived_wal, '') FROM pg_stat_archiver" | psql_in "$run-pg")
    [ -n "$last" ] && [ "$last" \> "$want" -o "$last" = "$want" ] && return 0
    sleep 1
  done
  fail "WAL was not archived (wanted $want, last $last)"
}
seed before 40

log "2. Base backup, more rows, recovery point, disaster"
docker exec -u postgres "$run-pg" /backup/bin/base-backup.sh
seed middle 25
sleep 1
T=$(echo "SELECT to_char(now() AT TIME ZONE 'utc', 'YYYY-MM-DD HH24:MI:SS.US') || '+00:00'" | psql_in "$run-pg")
echo "recovery point T = $T"
sleep 1
seed after 15
echo "DELETE FROM recordings" | psql_in "$run-pg" >/dev/null   # the disaster
echo "before=$(count "$run-pg" before) middle=$(count "$run-pg" middle) after=$(count "$run-pg" after) (all wiped)"
archive_all
docker exec "$run-pg" sh -c "ls /backup/wal | wc -l" | xargs echo "WAL segments archived:"

log "3. Off-site mirror, then lose the primary's backups"
docker run -d --name "$run-src" --network "$net" -e MINIO_ROOT_USER=minio -e MINIO_ROOT_PASSWORD=minio-secret \
  "$MINIO" server /data >/dev/null
docker run -d --name "$run-off" --network "$net" -e MINIO_ROOT_USER=minio -e MINIO_ROOT_PASSWORD=minio-secret \
  "$MINIO" server /data >/dev/null
for _ in $(seq 1 30); do mc ls src >/dev/null 2>&1 && mc ls off >/dev/null 2>&1 && break; sleep 1; done
mc mb off/sintade-backups >/dev/null
mc version enable off/sintade-backups >/dev/null
mc mirror --overwrite --quiet /work/backup off/sintade-backups >/dev/null
echo "off-site: $(mc ls --recursive off/sintade-backups | wc -l) objects"
# (the files belong to the postgres user in the container: remove them from a container)
docker run --rm -v "$work:/w" "$PG" sh -c 'rm -rf /w/backup/* /w/backup/.[!.]* 2>/dev/null; true'
docker rm -f "$run-pg" >/dev/null

log "4. Restore Postgres from the off-site copy to T"
restore_start=$(date +%s)
mc mirror --overwrite --quiet off/sintade-backups /work/backup >/dev/null
docker volume create "$run-restored" >/dev/null
docker run --rm --user postgres -v "$run-restored:/var/lib/postgresql/data" -v "$work/backup:/backup" \
  -v "$scripts:/backup/bin:ro" -e PGDATA=/var/lib/postgresql/data "$PG" \
  /backup/bin/restore-postgres.sh "$T"
docker run -d --name "$run-pg-restore" --network "$net" -e POSTGRES_PASSWORD=drill \
  -v "$run-restored:/var/lib/postgresql/data" -v "$work/backup:/backup:ro" "$PG" >/dev/null
for _ in $(seq 1 120); do
  state=$(echo "SELECT pg_is_in_recovery()" | psql_in "$run-pg-restore" 2>/dev/null || true)
  [ "$state" = "f" ] && break
  sleep 1
done
[ "${state:-}" = "f" ] || { docker logs "$run-pg-restore" 2>&1 | tail -20; fail "restore never finished recovery"; }
restore_secs=$(( $(date +%s) - restore_start ))
b=$(count "$run-pg-restore" before); m=$(count "$run-pg-restore" middle); a=$(count "$run-pg-restore" after)
echo "restored to T in ${restore_secs}s: before=$b middle=$m after=$a"
[ "$b" = 40 ] || fail "expected 40 'before' recordings, got $b"
[ "$m" = 25 ] || fail "expected 25 'middle' recordings (made before T), got $m"
[ "$a" = 0 ] || fail "expected 0 'after' recordings (made after T), got $a"
tables=$(echo "SELECT count(*) FROM information_schema.tables WHERE table_schema = 'public'" | psql_in "$run-pg-restore")
echo "restored database has $tables tables; migrations table rows: $(echo 'SELECT count(*) FROM _sqlx_migrations' | psql_in "$run-pg-restore" 2>/dev/null || echo n/a)"

log "5. Media bucket: mirror, wipe, restore, and undo a propagated delete"
mc mb src/media >/dev/null
mc version enable src/media >/dev/null || true
mc mb off/sintade-media >/dev/null
mc version enable off/sintade-media >/dev/null
for i in 1 2 3 4 5; do head -c $((20000 * i)) /dev/urandom > "$work/obj$i.bin"; done
: > "$work/sums.txt"
for i in 1 2 3 4 5; do
  mc cp --quiet "/work/obj$i.bin" "src/media/ws/rec/obj$i.bin" >/dev/null
  echo "$(sha256sum "$work/obj$i.bin" | cut -d' ' -f1)  obj$i.bin" >> "$work/sums.txt"
done
mc mirror --overwrite --remove --quiet src/media off/sintade-media >/dev/null
mc rm --recursive --force --quiet src/media >/dev/null          # the disaster
mc mirror --overwrite --quiet off/sintade-media src/media >/dev/null   # restore
for i in 1 2 3 4 5; do
  got=$(mc cat "src/media/ws/rec/obj$i.bin" | sha256sum | cut -d' ' -f1)
  want=$(grep "obj$i.bin" "$work/sums.txt" | cut -d' ' -f1)
  [ "$got" = "$want" ] || fail "media object $i differs after restore"
done
echo "media: 5/5 objects restored byte for byte"
# A mirror run after the disaster (with --remove) deletes the off-site copy too: versions bring it back.
mc rm --recursive --force --quiet src/media >/dev/null
mc mirror --overwrite --remove --quiet src/media off/sintade-media >/dev/null
[ "$(mc ls --recursive off/sintade-media | wc -l)" = 0 ] || fail "expected the propagated delete to empty the mirror"
mc undo --recursive --force off/sintade-media >/dev/null 2>&1 || mc undo off/sintade-media/ws/rec/ --recursive --force >/dev/null
n=$(mc ls --recursive off/sintade-media | wc -l)
[ "$n" = 5 ] || fail "versioning should have brought back 5 objects, found $n"
echo "media: propagated delete undone from versions (5 objects back)"

log "DRILL PASSED in $(( $(date +%s) - started ))s (Postgres restore ${restore_secs}s)"
