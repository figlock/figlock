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

# Regenerate the test vectors in docs/spec/vectors (only when a spec rule changes).
vectors:
    cd docs/spec/vectors && python3 -m venv .venv && .venv/bin/pip install -q -r requirements.txt && .venv/bin/python generate.py

# Check committed test vectors for drift and re-verify every case.
vectors-check:
    cd docs/spec/vectors && python3 -m venv .venv && .venv/bin/pip install -q -r requirements.txt && .venv/bin/python generate.py --check

# WCAG contrast check of the design tokens.
design-check:
    python3 -I design/check-contrast.py

# Everything CI runs: lint, tests, the design token check and the vector drift check.
check: lint test design-check vectors-check

# Supply-chain checks: advisories, licenses, bans, sources.
deny:
    if [ -f Cargo.toml ]; then cargo deny check; fi
