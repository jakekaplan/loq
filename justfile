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

# Enforce 100% coverage (uses cargo-llvm-cov)
coverage:
    cargo llvm-cov --all --workspace --lcov --output-path lcov.info --fail-under-lines 100

# CI-equivalent checks
ci: fmt-check clippy test coverage

# Benchmark runtime (requires hyperfine)
bench target=".":
    cargo build --release -p fence
    hyperfine --warmup 3 --runs 10 --ignore-failure \
        "./target/release/fence check {{ target }} --silent"
