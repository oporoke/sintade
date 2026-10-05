#!/usr/bin/env bash
# Deploys one image tag on this host (docs/design.md §19 "Deploy steps"). Run by the CI
# `deploy-production` / `deploy-staging` jobs over SSH, or by hand in an emergency:
#   /opt/sintade/deploy.sh <git sha>
# Order matters: pull everything first (a failed pull changes nothing), migrate, then restart the
# worker (graceful), the API, and the web container, checking health between steps.
set -euo pipefail
cd "$(dirname "$0")"

tag="${1:?usage: deploy.sh <image tag (git sha)>}"
# COMPOSE_PROJECT: a different project name for tests that must not touch a real stack.
compose=(docker compose ${COMPOSE_PROJECT:+-p "$COMPOSE_PROJECT"} -f compose.prod.yml --env-file .env)
profiles=(--profile edge --profile worker --profile data)
[ -n "${PROFILES:-}" ] && profiles=($(for p in $PROFILES; do echo --profile "$p"; done))
previous="$(grep -s '^IMAGE_TAG=' .env.deployed | cut -d= -f2 || true)"

step() { printf '\n== %s\n' "$*"; }
wait_healthy() { # wait_healthy <service> <seconds>
  local deadline=$((SECONDS + $2))
  until [ "$("${compose[@]}" "${profiles[@]}" ps --format '{{.Health}}' "$1" 2>/dev/null | head -1)" = healthy ]; do
    [ $SECONDS -lt $deadline ] || { echo "$1 did not become healthy" >&2; return 1; }
    sleep 2
  done
}
rollback_hint() {
  echo >&2
  echo "Deploy of $tag failed. To go back: ./deploy.sh ${previous:-<previous sha>}" >&2
  echo "(migrations are forward-only and compatible, so the previous image runs on the new schema)" >&2
}
trap 'rollback_hint' ERR

export IMAGE_TAG="$tag"

step "1. pull images for $tag"
if [ -z "${SKIP_PULL:-}" ]; then "${compose[@]}" "${profiles[@]}" pull; else echo "(SKIP_PULL set: using local images)"; fi

if printf '%s\n' "${profiles[@]}" | grep -qx data; then
  step "2. data services"
  "${compose[@]}" --profile data up -d
  wait_healthy postgres 90
fi

step "3. migrate the database (one run, from here)"
"${compose[@]}" "${profiles[@]}" run --rm --no-deps api migrate

if printf '%s\n' "${profiles[@]}" | grep -qx worker; then
  step "4. worker: SIGTERM, finish or re-queue the job in flight, start the new image"
  "${compose[@]}" --profile worker up -d --no-deps worker
fi

if printf '%s\n' "${profiles[@]}" | grep -qx edge; then
  step "5. api, then web and the edge"
  "${compose[@]}" --profile edge up -d --no-deps api
  wait_healthy api 90
  "${compose[@]}" --profile edge up -d
fi

step "6. smoke test"
domain="$(grep -s '^DOMAIN=' .env | cut -d= -f2-)"
port="$(grep -s '^HTTPS_PORT=' .env | cut -d= -f2-)"
curl_args=(-sS --fail --max-time 10)
[ -n "$(grep -s '^CADDY_TLS=.*internal' .env)" ] && curl_args+=(-k --resolve "$domain:${port:-443}:127.0.0.1")
probe() { # probe <path>: retried for a minute (Caddy may still be getting its certificate)
  local deadline=$((SECONDS + 60))
  until curl "${curl_args[@]}" "https://$domain${port:+:$port}$1" -o /dev/null; do
    [ $SECONDS -lt $deadline ] || { echo "$1 did not answer over TLS" >&2; return 1; }
    sleep 3
  done
}
probe /readyz
probe /healthz
echo "healthy: https://$domain/readyz"

echo "IMAGE_TAG=$tag" > .env.deployed
trap - ERR
step "deployed $tag (previous: ${previous:-none})"
