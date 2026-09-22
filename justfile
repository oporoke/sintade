check:
    cargo fmt --check
    cargo clippy --workspace --all-targets -- -D warnings

test:
    cargo test --workspace
