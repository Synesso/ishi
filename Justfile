default: dev

dev:
    cargo run

test:
    cargo test

check:
    cargo check

clippy:
    cargo clippy -- -D warnings

watch-test:
    cargo watch -x test
