check:
    cargo fmt --check
    cargo clippy --workspace --all-targets -- -D warnings

test:
    cargo test --workspace

deps-up:
    docker compose up -d --wait minio mailpit
    docker compose run --rm minio-init
