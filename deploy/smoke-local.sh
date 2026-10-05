#!/usr/bin/env bash
# Stands the production stack up on this machine from locally built images and checks it the way
# the deploy's smoke test and a first user would: TLS through Caddy, the probes, the migration,
# the security headers, a deep link, and a real sign-up -> recording -> chunk upload through the
# storage host. Tears everything down afterwards.
#
#   for i in api worker web; do docker build -f deploy/Dockerfile.$i -t ghcr.io/oporoke/sintade/$i:localtest .; done
#   deploy/smoke-local.sh            # IMAGE_TAG defaults to "localtest"
#
# Uses ports 8088/8443 and the domain sintade.localhost (curl resolves *.localhost itself).
set -euo pipefail
cd "$(dirname "$0")/.."
tag="${IMAGE_TAG:-localtest}"
dir="$(mktemp -d)"
export COMPOSE_PROJECT=sintade-smoke   # never the real or the dev project: `down -v` would remove its containers
[ -n "${KEEP:-}" ] || trap 'cd "$dir" 2>/dev/null && docker compose -p sintade-smoke -f compose.prod.yml --env-file .env --profile edge --profile worker --profile data down -v >/dev/null 2>&1; cd /; docker run --rm -v "$dir:/w" postgres:16 rm -rf /w/backups >/dev/null 2>&1; rm -rf "$dir"' EXIT

cp deploy/compose.prod.yml deploy/Caddyfile deploy/deploy.sh "$dir/"
cp -r deploy/backup "$dir/backup"
chmod 777 "$dir"
domain=sintade.localhost
secret="$(openssl rand -base64 64 | tr -d '\n')"
cat > "$dir/.env" <<ENV
DOMAIN=$domain
ACME_EMAIL=ops@sintade.localhost
POSTGRES_PASSWORD=smoke-pg-pass
MINIO_ROOT_USER=smokeroot
MINIO_ROOT_PASSWORD=smoke-root-password
BACKUP_DIR=$dir/backups
S3_BUCKET=sintade-smoke
S3_ACCESS_KEY=smokeapp
S3_SECRET_KEY=smoke-app-secret-key
CADDY_TLS=tls internal
HTTPS_PORT=8443
HTTP_PORT=8088
APP_ENV_FILE=$dir/app.env
IMAGE_TAG=$tag
ENV
mkdir -p "$dir/backups"
chmod 777 "$dir/backups"
cat > "$dir/app.env" <<ENV
DATABASE_URL=postgres://app:smoke-pg-pass@postgres:5432/sintade
PUBLIC_BASE_URL=https://$domain:8443
S3_ENDPOINT=http://minio:9000
S3_PUBLIC_ENDPOINT=https://storage.$domain:8443
S3_BUCKET=sintade-smoke
S3_ACCESS_KEY=smokeapp
S3_SECRET_KEY=smoke-app-secret-key
SMTP_URL=smtp://mailpit-not-here:1025
SESSION_SECRET=$secret
RUST_LOG=info
APP_ENV=smoke
WORKER_SCRATCH_DIR=/var/lib/sintade/scratch
ENV

cd "$dir"
SKIP_PULL=1 ./deploy.sh "$tag"

k=(-sS --max-time 15 -k)
base="https://$domain:8443"
fail() { echo "SMOKE FAILED: $*" >&2; exit 1; }
echo; echo "== checks"
code() { curl "${k[@]}" -o /dev/null -w '%{http_code}' "$@"; }
[ "$(code "$base/readyz")" = 200 ] || fail "readyz over TLS"
echo "readyz 200 over TLS"
[ "$(code "$base/s/abcdefghijkl")" = 200 ] || fail "deep link"
echo "deep link /s/<slug> serves the app"
[ "$(code "$base/api/v1/me")" = 401 ] || fail "api through web"
echo "api reachable through the web container (401 without a session)"
headers="$(curl "${k[@]}" -D - -o /dev/null "$base/")"
for h in content-security-policy strict-transport-security x-content-type-options x-frame-options permissions-policy; do
  echo "$headers" | grep -qi "^$h:" || { echo "MISSING header $h" >&2; exit 1; }
done
echo "security headers present"
echo "$headers" | grep -qi '^content-security-policy:.*storage.sintade.localhost' && echo "CSP allows the storage host"

# A user, a recording, one chunk uploaded to the storage host with a presigned URL.
jar="$dir/jar"
email="smoke-$$@example.com"
curl "${k[@]}" -X POST "$base/api/v1/auth/register" -H 'content-type: application/json' -H 'x-forwarded-for: 198.51.100.77' \
  -d "{\"email\":\"$email\",\"password\":\"correct-horse-battery-staple-42\",\"display_name\":\"Smoke\"}" -o /dev/null -w 'register %{http_code}\n'
curl "${k[@]}" -c "$jar" -X POST "$base/api/v1/auth/login" -H 'content-type: application/json' \
  -d "{\"email\":\"$email\",\"password\":\"correct-horse-battery-staple-42\"}" -o /dev/null -w 'login %{http_code}\n'
csrf="$(awk '$6=="sintade_csrf"{print $7}' "$jar" | head -1)"
rec="$(curl "${k[@]}" -b "$jar" -X POST "$base/api/v1/recordings" -H 'content-type: application/json' -H "x-csrf-token: $csrf" \
  -d '{"mime_type":"video/webm;codecs=vp9","has_system_audio":false,"has_mic":false,"has_camera":false}')"
take="$(echo "$rec" | python3 -c 'import json,sys; print(json.load(sys.stdin)["take_id"])')"
url="$(curl "${k[@]}" -b "$jar" -X POST "$base/api/v1/takes/$take/chunks/0/url" -H "x-csrf-token: $csrf" | python3 -c 'import json,sys; print(json.load(sys.stdin)["urls"][0]["url"])')"
echo "presigned host: $(echo "$url" | sed -E 's#(https://[^/]+)/.*#\1#')"
head -c 4096 /dev/urandom > "$dir/chunk.bin"
put="$(curl "${k[@]}" -X PUT --data-binary @"$dir/chunk.bin" -o /dev/null -w '%{http_code}' "$url")"
echo "chunk PUT to the storage host: $put"
[ "$put" = 200 ] || fail "the chunk PUT to the storage host answered $put"
sha="$(sha256sum "$dir/chunk.bin" | cut -d' ' -f1)"
ack="$(curl "${k[@]}" -b "$jar" -X POST "$base/api/v1/takes/$take/chunks/0/ack" -H 'content-type: application/json' -H "x-csrf-token: $csrf" \
  -d "{\"size_bytes\":4096,\"sha256\":\"$sha\"}" -o /dev/null -w '%{http_code}')"
echo "ack: $ack"
[ "$ack" = 200 ] || fail "the chunk ack answered $ack"
echo "chunk acknowledged: the whole path (browser -> Caddy -> MinIO -> API -> Postgres) works"
echo; echo "PROD SMOKE PASSED"
