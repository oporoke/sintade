set dotenv-load

check:
    cargo fmt --check
    cargo clippy --workspace --all-targets -- -D warnings
    cargo sqlx prepare --check --workspace
    just openapi-check
    cd web && npm run lint
    cd extension && npm run lint

test:
    cargo test --workspace
    cd web && npm test -- --watch=false
    cd extension && npm test

e2e:
    cd web && npx playwright test
    cd extension && npm run e2e

# M3 demo: films a 2-minute recording and a crash/recovery per browser into web/demo-output/
demo-m3 *args:
    cd web && DEMO=1 npx playwright test e2e/m3-demo.spec.ts --workers=1 {{args}}

# M4 demo: a 10-minute recording with the network cut for 60 s; films + result.json per browser
# into web/demo-output/m4/ (~12 min per browser; WebKit skips: no MediaRecorder on Linux)
demo-m4 *args:
    cd web && DEMO=1 npx playwright test e2e/m4-demo.spec.ts --workers=1 {{args}}

# Day 50 performance check: a 30-min 1080p take through the MP4 step on 4 cores (needs ~3 GB in
# target/fixtures; takes ~25 min incl. generating the input). Must finish in <= 15 min.
perf-long minutes="30":
    docs/fixtures/generate-long.sh {{minutes}}
    cargo build -q -p worker
    /usr/bin/time -v taskset -c 0-3 target/debug/worker transcode target/fixtures/vp9_opus_{{minutes}}min_1080p.webm target/fixtures/out_{{minutes}}min.mp4

# M5 demo: records 15 s, waits for the worker's MP4 and the "ready" email. Needs `just api`-less
# setup (Playwright starts the API) but `just worker` running. Films into web/demo-output/m5/
demo-m5 *args:
    cd web && DEMO=1 npx playwright test e2e/m5-demo.spec.ts --workers=1 {{args}}

# M6 demo: the full record -> share -> watch -> library -> download -> trash loop on Chromium and
# Firefox (~1 min each). Needs `just worker` running. Films into web/demo-output/m6/
demo-m6 *args:
    cd web && DEMO=1 npx playwright test e2e/m6-demo.spec.ts --workers=1 {{args}}

# The browser extension (Manifest V3, Chrome and Edge): build into extension/dist/, then load it
# unpacked at chrome://extensions (Developer mode -> Load unpacked).
ext-build:
    cd extension && npm run build

ext-watch:
    cd extension && npm run watch

# Secrets in the whole git history (gitleaks in Docker).
secrets:
    docker run --rm -v "$PWD:/repo" zricethezav/gitleaks:latest detect --source /repo --config /repo/.gitleaks.toml --redact --no-banner

# Dependency advisories (needs `cargo install cargo-audit`).
audit:
    cargo audit --deny warnings
    cd web && npm audit --audit-level=high
    cd extension && npm audit --audit-level=high

# The backup/restore drill: restores Postgres to a point in time and the media bucket from an
# off-site copy, in throwaway Docker containers (deploy/backup/restore-drill.sh).
restore-drill:
    deploy/backup/restore-drill.sh

# The production stack on this machine from locally built images, through Caddy's own TLS, with a
# first user's sign-up and chunk upload (deploy/smoke-local.sh). Build the images first (see the
# script's header). Never touches the dev stack: it runs as its own compose project.
prod-smoke:
    deploy/smoke-local.sh

# OWASP ZAP baseline of the web image and an API scan from the OpenAPI contract, against the
# running local stack (API on :8080). Reports go to target/zap/. See scripts/zap-scan.sh.
zap:
    scripts/zap-scan.sh

deps-up:
    docker compose up -d --wait postgres minio mailpit
    docker compose run --rm minio-init

db-migrate:
    sqlx migrate run

api:
    cargo run -p api

worker:
    cargo run -p worker

web:
    cd web && npm ci && npm start

migration name:
    sqlx migrate add {{name}}

# Regenerate the API contract and the frontend DTOs generated from it.
openapi:
    cargo run -q -p api -- openapi > docs/api/openapi.json
    cd web && npx openapi-typescript ../docs/api/openapi.json -o src/app/api/schema.ts

# Fail if the committed contract or DTOs are stale (run `just openapi` to fix).
openapi-check:
    #!/usr/bin/env bash
    set -euo pipefail
    tmp=$(mktemp -d)
    cargo run -q -p api -- openapi > "$tmp/openapi.json"
    diff -u docs/api/openapi.json "$tmp/openapi.json"
    (cd web && npx openapi-typescript ../docs/api/openapi.json -o "$tmp/schema.ts" > /dev/null)
    diff -u web/src/app/api/schema.ts "$tmp/schema.ts"
