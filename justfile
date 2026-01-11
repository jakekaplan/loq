set shell := ["bash", "-eu", "-o", "pipefail", "-c"]

# Format the workspace
fmt:
    cargo fmt

# Check formatting
fmt-check:
    cargo fmt --check

# Run clippy with warnings as errors
clippy:
    cargo clippy --all-targets --all-features -- -D warnings

# Quick local checks
check: fmt-check clippy

# Run tests
test:
    cargo test --all

# Enforce 95% coverage (uses cargo-llvm-cov)
coverage:
    cargo llvm-cov --workspace --lcov --output-path lcov.info --fail-under-lines 95

# CI-equivalent checks
ci: fmt-check clippy test coverage

# Run criterion benchmarks
bench-criterion:
    cargo bench -p loq_fs

# Benchmark against a public GitHub repo (requires hyperfine)
bench repo:
    #!/usr/bin/env bash
    set -euo pipefail
    cargo build --release -p loq
    TMPDIR=$(mktemp -d)
    trap "rm -rf $TMPDIR" EXIT
    git clone --depth 1 "{{ repo }}" "$TMPDIR/repo"
    hyperfine --warmup 3 --runs 10 --ignore-failure \
        "./target/release/loq check $TMPDIR/repo --silent"
