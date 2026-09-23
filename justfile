set dotenv-load

check:
    cargo fmt --check
    cargo clippy --workspace --all-targets -- -D warnings
    cargo sqlx prepare --check --workspace
    cd web && npm run lint

test:
    cargo test --workspace
    cd web && npm test -- --watch=false

e2e:
    cd web && npx playwright test

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
