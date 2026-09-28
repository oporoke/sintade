set dotenv-load

check:
    cargo fmt --check
    cargo clippy --workspace --all-targets -- -D warnings
    cargo sqlx prepare --check --workspace
    just openapi-check
    cd web && npm run lint

test:
    cargo test --workspace
    cd web && npm test -- --watch=false

e2e:
    cd web && npx playwright test

# M3 demo: films a 2-minute recording and a crash/recovery per browser into web/demo-output/
demo-m3 *args:
    cd web && DEMO=1 npx playwright test e2e/m3-demo.spec.ts --workers=1 {{args}}

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
