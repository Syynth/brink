# Setup git hooks
setup:
    git config core.hooksPath .githooks

# Type-check the workspace
check:
    cargo check --workspace

# Run all tests
test:
    cargo test --workspace

# Run all lints (fmt check + clippy)
lint:
    cargo fmt --all -- --check
    cargo clippy --workspace --all-targets -- -D warnings

# Auto-fix what can be fixed
fix:
    cargo fmt --all
    cargo clippy --workspace --all-targets --fix --allow-dirty
