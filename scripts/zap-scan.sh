#!/usr/bin/env bash
# OWASP ZAP against the local stack: a passive baseline of the production web image (nginx with
# its security headers, proxying /api to the running API) and a ZAP API scan generated from
# docs/api/openapi.json. Needs Docker, `just db-migrate` done and the API running on :8080
# (`just api`). Reports: target/zap/{web-baseline,api-scan}.{html,json}. Exit code 1 on any FAIL.
set -euo pipefail
cd "$(dirname "$0")/.."

out="$PWD/target/zap"
mkdir -p "$out"
chmod 777 "$out"
cp docs/api/openapi.json "$out/openapi.json"
cp scripts/zap-baseline.conf "$out/zap-baseline.conf"

curl -sf http://localhost:8080/healthz >/dev/null || { echo "start the API first: just api" >&2; exit 2; }

docker build -q -f deploy/Dockerfile.web -t sintade-web:zap .
docker rm -f sintade-web-zap >/dev/null 2>&1 || true
docker run -d --name sintade-web-zap --add-host host.docker.internal:host-gateway \
  -e API_UPSTREAM=host.docker.internal:8080 -p 8081:80 sintade-web:zap >/dev/null
trap 'docker rm -f sintade-web-zap >/dev/null 2>&1 || true' EXIT
sleep 2

zap=ghcr.io/zaproxy/zaproxy:stable
common=(--rm --add-host host.docker.internal:host-gateway -v "$out:/zap/wrk:rw")

status=0
docker run "${common[@]}" "$zap" zap-baseline.py -t http://host.docker.internal:8081 \
  -J web-baseline.json -r web-baseline.html -m 3 -c zap-baseline.conf || status=1
docker run "${common[@]}" "$zap" zap-api-scan.py -t /zap/wrk/openapi.json -f openapi \
  -O http://host.docker.internal:8080 -J api-scan.json -r api-scan.html -T 8 || status=1
exit $status
