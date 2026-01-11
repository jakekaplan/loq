# Contributing to loq

Thanks for your interest in contributing to loq!

## Development Setup

1. Install Rust via [rustup](https://rustup.rs/)
2. Clone the repository
3. Run `cargo build` to compile

## Before Submitting a PR

Run these checks locally (they mirror CI):

```bash
# Format
cargo fmt --all -- --check

# Lint
cargo clippy --all-targets --all-features -- -D warnings

# Test
cargo test --all

# Self-check (loq checks loq)
cargo run -p loq -- check .
```

If you have [just](https://github.com/casey/just) installed:

```bash
just ci
```

## Code Guidelines

- **No unsafe code**: All crates use `#![forbid(unsafe_code)]`
- **Test coverage**: Maintain 95%+ coverage
- **Error handling**: Use `thiserror` in libraries, `anyhow` in the CLI
- **Documentation**: Add rustdoc comments to public items

## Project Structure

```
crates/
  loq_core/   # Domain logic (config, rules, reporting)
  loq_fs/     # Filesystem operations (walking, counting)
  loq_cli/    # CLI interface
```

## Running Benchmarks

```bash
# Criterion benchmarks
cargo bench -p loq_fs

# Real-world benchmark (requires hyperfine)
just bench https://github.com/owner/repo
```

## Commit Messages

Write clear, concise commit messages. No specific format is required, but focus on the "why" rather than the "what".
