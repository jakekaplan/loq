# loq

[![CI](https://github.com/jakekaplan/loq/actions/workflows/ci.yml/badge.svg)](https://github.com/jakekaplan/loq/actions/workflows/ci.yml)
[![codecov](https://codecov.io/gh/jakekaplan/loq/graph/badge.svg)](https://codecov.io/gh/jakekaplan/loq)
[![PyPI](https://img.shields.io/pypi/v/loq)](https://pypi.org/project/loq/)
[![Crates.io](https://img.shields.io/crates/v/loq)](https://crates.io/crates/loq)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)

An electric fence for LLMs (and humans too). Written in Rust.

## Why loq?

Big files burn tokens. That means slower responses, higher costs, and context rot where large files become dumping grounds that overwhelm both LLMs and humans.

You can tell an LLM what to do, but the only way to **guarantee** it listens is with hard constraints. loq provides that constraint: a fast, dead-simple way to enforce file size limits.

Linters like Ruff and ESLint check correctness. loq checks size. One thing: line counts (`wc -l` style). No parsers, no plugins, language agnostic. One tool for your entire polyglot monorepo.

## Getting started

### Installation

```bash
# With uv (recommended)
uv tool install loq

# With pip
pip install loq

# With cargo
cargo install loq
```

### Usage

```bash
loq                                # Check current directory (500 line default)
loq check src/ lib/                # Check specific paths
git diff --name-only | loq check - # Check files from stdin
```

### Pre-commit

```yaml
repos:
  - repo: https://github.com/jakekaplan/loq
    rev: v0.1.0a4
    hooks:
      - id: loq
```

### LLM-friendly output

Output is designed to be token-efficient:

```
✖  1_427 > 500   src/components/Dashboard.tsx
✖    892 > 500   src/utils/helpers.py
2 violations (14ms)
```

Use `loq -v` for additional context:

```
✖  1_427 > 500   src/components/Dashboard.tsx
                  └─ rule: max-lines=500 (match: **/*.tsx)
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

[[rules]]
path = "tests/**"
max_lines = 600
```

### Fix guidance

Add `fix_guidance` to show project-specific instructions with violations—useful when piping to LLMs:

```toml
fix_guidance = "Split large files: helpers → src/utils/, types → src/types/"
```

### Baseline

Existing large files? Baseline them and ratchet down over time:

```bash
loq init       # Create loq.toml first
loq baseline   # Add rules for files over the limit
```

Run periodically. It automatically:
- **Tightens** limits when files shrink
- **Removes** rules when files drop below the threshold
- **Ignores** files that grew (use `--allow-growth` to override)

Use `--threshold 300` to set a custom limit.

## Contributing

Contributions are welcome! See [CONTRIBUTING.md](CONTRIBUTING.md) for development setup and guidelines.

## License

This project is licensed under the [MIT License](LICENSE).
