# Contributing to loq

Thanks for your interest in contributing!

## Development setup

1. Install Rust via [rustup](https://rustup.rs/)
2. Clone the repository
3. Run `cargo build` to compile

## Before submitting a PR

Run these checks locally (they mirror CI):

```bash
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all
cargo run -p loq -- check .   # loq checks loq
```

Or with [just](https://github.com/casey/just):

```bash
just ci
```

## Testing

**Unit tests** live in `mod tests` blocks within each module.

**Integration tests** use [insta](https://insta.rs/) for snapshot testing. Snapshots live in `crates/loq_cli/tests/snapshots/`.

To update snapshots after intentional output changes:

```bash
cargo insta review
```

**Coverage** is enforced at 95%+ line coverage. Check locally with:

```bash
cargo llvm-cov --workspace --fail-under-lines 95
```

## Code guidelines

- **No unsafe code** - all crates use `#![forbid(unsafe_code)]`
- **Strict linting** - pedantic clippy + restriction lints (no prints, no dbg!, etc.)
- **Error handling** - `thiserror` in libraries, `anyhow` in CLI
- **Documentation** - rustdoc comments on public items

## Project structure

```
crates/
  loq_core/   # Domain logic (config, rules, reporting)
  loq_fs/     # Filesystem operations (walking, counting, caching)
  loq_cli/    # CLI binary
python/       # Python package wrapper (PyPI distribution)
```

## Benchmarks

```bash
cargo bench -p loq_fs                        # Criterion microbenchmarks
just bench https://github.com/astral-sh/ruff # Real-world benchmark (requires hyperfine)
```
