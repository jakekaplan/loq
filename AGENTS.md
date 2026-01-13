# AGENTS.md

Guidance for AI agents working on this codebase.

## What is loq?

loq enforces file line limits. That's it. It's an "electric fence" that keeps files from growing too large - critical for LLM-assisted development where big files burn context and degrade output quality.

**Philosophy**: Dead simple, fast, language agnostic. No parsing, no plugins. Just line counts.

**North star**: [Ruff](https://github.com/astral-sh/ruff) - a Rust-powered Python tool with excellent DX. We aim for that same quality: fast, helpful errors, zero-config defaults, thoughtful CLI design.

## Project structure

```
crates/
  loq_core/     # Pure logic: config parsing, rule matching, reporting
  loq_fs/       # Filesystem: directory walking, line counting, caching
  loq_cli/      # CLI: argument parsing, output formatting, commands
python/         # Python package wrapper for PyPI
```

**Where things go**:
- Business logic with no I/O → `loq_core`
- Anything touching the filesystem → `loq_fs`
- User-facing CLI concerns → `loq_cli`
- Keep crates focused. If something doesn't fit cleanly, that's a smell.

## Writing code

### File size

loq enforces its own limits. **Keep files under 500 lines.** Run `cargo run -p loq -- check .` to verify.

When splitting files:
- **Name files by what they do**, not generic names like `utils.rs` or `helpers.rs`
- **Tests can split first** - move `mod tests` to a separate `tests.rs` file if the module is getting long
- **One concept per file** - if a file does two things, it's two files

### Rust style

- **Simple over clever** - readable code beats clever code
- **Small functions** - if it needs a comment explaining what it does, extract it
- **No unsafe** - all crates use `#![forbid(unsafe_code)]`
- **Minimize allocations** - avoid unnecessary `.clone()`, prefer borrowing
- **Use iterators** - prefer `.iter().map().filter()` over manual loops when clearer
- **Prefer let chains** - use `if let` combined with `&&` over nested `if let` statements to reduce indentation
- **Follow existing patterns** - check neighboring files for style conventions before writing new code
- **Reuse before writing** - avoid writing significant new code; look for existing utilities first

### Error handling

- **Libraries** (`loq_core`, `loq_fs`): typed errors with `thiserror`
- **CLI** (`loq_cli`): `anyhow` with `.context()` for user-facing errors
- **Never panic** in production paths - no `unwrap()` or `expect()` unless the invariant is documented
- **Encode constraints in types** - avoid `panic!`, `unreachable!`, or `.unwrap()` by designing types that make invalid states unrepresentable

### Testing

We maintain **>95% line coverage**. This is enforced in CI. Every PR must include tests.

**All changes must be tested.** If you're not testing your changes, you're not done. If you didn't run the tests, your code does not work.

**Unit tests**: In `mod tests { }` blocks within the module. Test the logic, not the implementation.

**Integration tests**: In `crates/*/tests/`. Test real CLI behavior.

**Snapshot testing**: We use [insta](https://insta.rs/) for CLI output. Snapshots live in `crates/loq_cli/tests/snapshots/`. When output changes intentionally:

```bash
cargo insta review
```

**Test philosophy**:
- If you add a feature, add a test
- If you fix a bug, add a regression test
- If you change output, update the snapshot
- Tests should be fast and deterministic

## Development commands

```bash
cargo fmt --all                                              # Format
cargo clippy --all-targets --all-features -- -D warnings     # Lint (strict)
cargo test --all                                             # Test
cargo llvm-cov --workspace --fail-under-lines 95             # Coverage check
cargo run -p loq -- check .                                  # Self-check (loq checks loq)
```

Or with `just`:

```bash
just ci    # Run all CI checks locally
```

**Always run `uvx prek run -a` at the end of a task** to verify all checks pass.

## CI

CI runs on ubuntu, macos, and windows. Before pushing:

1. `cargo fmt --all -- --check`
2. `cargo clippy --all-targets --all-features -- -D warnings`
3. `cargo test --all`
4. Coverage ≥95%

## Adding features

1. Start in the right crate (see "Where things go")
2. Write the test first or alongside the code
3. Keep the PR focused - one feature/fix per PR
4. Update snapshots if CLI output changes
5. Run `cargo run -p loq -- check .` - loq enforces its own limits

## Dependencies

- Prefer widely-used, maintained crates
- Workspace deps go in root `Cargo.toml`
- Run `cargo audit` and `cargo deny check` when adding deps
