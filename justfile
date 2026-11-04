# Run `just` with no arguments to list recipes.
default:
    @just --list

# One-time setup: install pinned tools and enable the git hooks.
setup:
    mise install
    git config core.hooksPath .githooks

# Format all code.
fmt:
    if [ -f Cargo.toml ]; then cargo fmt --all; fi
    if [ -f pnpm-workspace.yaml ]; then pnpm -r --if-present run fmt; fi

# Lint all code. Warnings are errors.
lint:
    if [ -f Cargo.toml ]; then cargo fmt --all --check && cargo clippy --workspace --all-targets -- -D warnings; fi
    if [ -f pnpm-workspace.yaml ]; then pnpm -r --if-present run lint; fi

# Run all tests.
test:
    if [ -f Cargo.toml ]; then cargo test --workspace; fi
    if [ -f pnpm-workspace.yaml ]; then pnpm -r --if-present run test; fi

# Everything CI runs: lint plus tests.
check: lint test
