# loq

Enforce file size constraints.

loq is a fast, minimal tool that keeps codebases LLM-context-friendly and human-maintainable by enforcing hard per-file line limits. The name visually resembles "loc" (lines of code).

LLMs happily generate big files. Big files burn context, slow reviews, and become dumping grounds. loq provides a hard constraint that stops files from growing too large.

## Install

```bash
cargo install loq
```

## Quick start

```bash
# zero-config: uses built-in defaults
loq

# explicit check
loq check src/

# stdin list
git diff --cached --name-only | loq check -
```

## Output

Violations are structured and LLM-parseable:

```
error[max-lines]: src/utils.py: 512 lines (limit: 500, +12 over)
```

Summary:

```
10 files checked, 2 skipped, 5 passed, 2 errors, 1 warning (15ms)
```

## Config

Create a config with defaults:

```bash
loq init
```

Baseline a legacy repo (exempts current errors, blocks new ones):

```bash
loq init --baseline
```

Config discovery walks upward from each file’s directory and uses the nearest `loq.toml`. Patterns are matched against paths relative to the config directory.

`respect_gitignore` defaults to true and applies the root `.gitignore` when scanning. The built-in defaults do not add any exclude patterns.

## Development

Install Rust (once):

```bash
rustup default stable
```

Common tasks:

```bash
# build
cargo build

# run the CLI locally
cargo run -p loq -- check .

# install locally
cargo install --path crates/loq_cli

# quick checks (fmt + clippy)
just check

# benchmark against a public repo
just bench https://github.com/astral-sh/ruff
```

Enable git hooks in this repo:

```bash
git config core.hooksPath .githooks
```

## Default `loq.toml`

```toml
default_max_lines = 500

respect_gitignore = true

exclude = []

exempt = []

# Last match wins. Put general rules first and overrides later.
[[rules]]
path = "**/*.tsx"
max_lines = 300
severity = "warning"

[[rules]]
path = "tests/**/*"
max_lines = 500
```

## License

MIT.
