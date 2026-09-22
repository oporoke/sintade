set dotenv-load

check:
    cargo fmt --check
    cargo clippy --workspace --all-targets -- -D warnings
    cargo sqlx prepare --check --workspace

test:
    cargo test --workspace

deps-up:
    docker compose up -d --wait postgres minio mailpit
    docker compose run --rm minio-init

db-migrate:
    sqlx migrate run

api:
    cargo run -p api
