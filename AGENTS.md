# AGENTS.md

Agent guidance for this Rust repository.

## Project Overview

`loq` is a Rust CLI tool organized as a workspace:
- `loq_core` - Core logic (library)
- `loq_fs` - Filesystem operations (library)
- `loq_cli` - Command-line interface (binary)

## Dev Environment

Rust toolchain should be pinned via `rust-toolchain.toml`. If missing, use stable with rustfmt and clippy components.

### Primary Commands

```bash
# Format
cargo fmt --all

# Lint
cargo clippy --all-targets --all-features -- -D warnings

# Test
cargo test --all

# Coverage (95% minimum)
cargo llvm-cov --workspace --lcov --output-path lcov.info --fail-under-lines 95

# Benchmark against a public repo (requires hyperfine)
just bench https://github.com/astral-sh/ruff
```

## CI Parity

Before committing, ensure these pass locally:

1. `cargo fmt --all -- --check`
2. `cargo clippy --all-targets --all-features -- -D warnings`
3. `cargo test --all`
4. `cargo test --doc`

CI runs on ubuntu, macos, and windows.

## Dependency & Security Policy

- Prefer widely used, actively maintained crates
- Run supply chain checks when dependencies change:
  - `cargo audit`
  - `cargo deny check`
- Workspace dependencies are declared in root `Cargo.toml`

## Code Conventions

### Error Handling

- **Libraries** (`loq_core`, `loq_fs`): Use typed errors with `thiserror`
- **Binary** (`loq_cli`): Use `anyhow` at the boundary with `.context()`
- Never `unwrap()` or `expect()` in production paths unless there is a clear invariant (comment why)

### Logging

- Use `tracing` for structured logs/spans
- Never log secrets

### Testing

- Unit tests in `mod tests { ... }` blocks
- Integration tests in `crates/*/tests/`
- Maintain 95%+ line coverage

### Style

- Run `cargo fmt` before committing
- Keep functions focused and readable
- Avoid unnecessary allocations/clones in hot paths

## PR Expectations

- Add or update tests for behavior changes
- Keep public API changes intentional and documented
- Keep diffs focused; avoid drive-by refactors
- All CI checks must pass

## Workspace Structure

```
.
├── Cargo.toml              # workspace manifest
├── crates/
│   ├── loq_core/         # core library
│   ├── loq_fs/           # filesystem operations
│   └── loq_cli/          # CLI binary
└── .github/workflows/ci.yml
```

## Adding a New Crate

1. Create `crates/new_crate/` with `src/lib.rs` or `src/main.rs`
2. Add `Cargo.toml` using `package.workspace = true` for shared fields
3. Add crate name to `members` in root `Cargo.toml`
4. Reference workspace deps: `dep_name.workspace = true`
