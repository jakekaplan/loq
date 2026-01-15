# loq

[![CI](https://github.com/jakekaplan/loq/actions/workflows/ci.yml/badge.svg)](https://github.com/jakekaplan/loq/actions/workflows/ci.yml)
[![codecov](https://codecov.io/gh/jakekaplan/loq/graph/badge.svg)](https://codecov.io/gh/jakekaplan/loq)
[![PyPI](https://img.shields.io/pypi/v/loq)](https://pypi.org/project/loq/)
[![Crates.io](https://img.shields.io/crates/v/loq)](https://crates.io/crates/loq)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)

An electric fence for LLMs (and humans too). Written in Rust.
loq enforces file line limits: fast, zero-config, and language agnostic.

## Quickstart

```bash
# With uv (recommended)
uv tool install loq

# With pip
pip install loq

# With cargo
cargo install loq
```

```bash
loq                                # Check current directory (500 line default)
loq check src/ lib/                # Check specific paths
git diff --name-only | loq check - # Check files from stdin
loq accept-defeat                  # Bump limits for current violations
```

## Why loq?

- Hard limits on file size to prevent context rot
- One metric: line counts (`wc -l` style)
- No parsers, no plugins, no config required
- LLM-friendly minimal output and fast Rust core

LLM-friendly output is token-efficient by default:

```
✖  1_427 > 500   src/components/Dashboard.tsx
✖    892 > 500   src/utils/helpers.py
2 violations (14ms)
```

Use `loq -v` for more context:

```
✖  1_427 > 500   src/components/Dashboard.tsx
                  └─ rule: max-lines=500 (match: **/*.tsx)
```

If you pipe results to an LLM, `fix_guidance` adds project-specific instructions
to each violation:

```toml
fix_guidance = "Split large files: helpers → src/utils/, types → src/types/"
```

## Configuration

loq works zero-config. Run `loq init` to customize:

```toml
default_max_lines = 500       # files not matching any rule
respect_gitignore = true      # skip .gitignore'd files
exclude = [".git/**", "**/generated/**", "*.lock"]

[[rules]]                     # last match wins, ** matches any path
path = "**/*.tsx"
max_lines = 300
```

## Managing legacy files

Existing large files? Baseline them and ratchet down over time:

```bash
loq init       # Create loq.toml first
loq baseline   # Add rules for files over the limit
```

Run periodically. It tightens limits as files shrink, removes rules once files
are under the threshold, and ignores files that grew (use `--allow-growth` to
override). Use `--threshold 300` to set a custom limit.

Need to ship while files are still too big? Accept defeat creates or updates
exact-path rules for the files currently failing checks:

```bash
loq accept-defeat                # Use default buffer of 100 lines
loq accept-defeat src/legacy.rs  # Only update one file
loq accept-defeat --buffer 50    # Add 50 lines above current size
```

It never edits glob rules; it only adds exact-path overrides at the end of the
`[[rules]]` list.

## Automation

```yaml
repos:
  - repo: https://github.com/jakekaplan/loq
    rev: v0.1.0a4
    hooks:
      - id: loq
```

## Contributing

Contributions are welcome! See [CONTRIBUTING.md](CONTRIBUTING.md) for development setup and guidelines.

## License

This project is licensed under the [MIT License](LICENSE).
