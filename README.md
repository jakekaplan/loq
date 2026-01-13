# loq

[![CI](https://github.com/jakekaplan/loq/actions/workflows/ci.yml/badge.svg)](https://github.com/jakekaplan/loq/actions/workflows/ci.yml)
[![codecov](https://codecov.io/gh/jakekaplan/loq/graph/badge.svg)](https://codecov.io/gh/jakekaplan/loq)
[![PyPI](https://img.shields.io/pypi/v/loq)](https://pypi.org/project/loq/)
[![Crates.io](https://img.shields.io/crates/v/loq)](https://crates.io/crates/loq)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)

An electric fence for LLMs (and humans too).

## Why file size matters

Big files mean more tokens. More tokens mean:

- **Slower responses** - LLMs take longer to process what they don't need
- **Higher costs** - you pay per token
- **Context rot** - large files become dumping grounds that overwhelm both LLMs and humans

loq stops the sprawl before it starts.

## Why loq?

LLMs are great at generating code, but sometimes they go off the rails. You can tell an LLM what to do, but the only way to **guarantee** it listens is with feedback loops and hard constraints. loq provides that constraint: a fast, dead-simple way to enforce file size limits.

Linters like Ruff and ESLint check correctness. loq checks size. It does one thing: enforce line counts (`wc -l` style). No parsers, no plugins, language agnostic. One tool for your entire polyglot monorepo.

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
loq                                        # Check current directory (zero-config, 500 line default)
loq check src/ lib/                        # Check specific paths
git diff --name-only | loq check -         # Check files from stdin
```

### Pre-commit

```yaml
repos:
  - repo: https://github.com/jakekaplan/loq
    rev: v0.1.0a3
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

### Custom guidance

Add `fix_guidance` to display instructions when violations occur:

```toml
default_max_lines = 500
fix_guidance = """
Split this file following our conventions:
- Extract helper functions to src/utils/
- Move types to src/types/
- Keep components under 300 lines
"""
```

Output:

```
✖    892 > 500   src/utils/helpers.py
1 violation (8ms)

Split this file following our conventions:
- Extract helper functions to src/utils/
- Move types to src/types/
- Keep components under 300 lines
```

This is especially useful when piping to LLMs—include project-specific refactoring patterns and the LLM gets actionable context alongside the violations.

### Baseline

Have a codebase with existing large files? Baseline them:

```bash
loq init       # Create loq.toml first
loq baseline   # Add rules for files over the limit
```

Run `loq baseline` periodically to ratchet down. It automatically:
- **Adds** rules for new violations
- **Updates** rules when files shrink (tightens the limit)
- **Removes** rules when files drop below the threshold

Files that grow beyond their baseline are left unchanged by default—increasing limits defeats the purpose of the fence. If you must, use `--allow-growth`:

```bash
loq baseline --allow-growth   # Also update limits for files that grew
```

Use `--threshold` to override the default limit:

```bash
loq baseline --threshold 300
```

## Contributing

Contributions are welcome! See [CONTRIBUTING.md](CONTRIBUTING.md) for development setup and guidelines.

## License

This project is licensed under the [MIT License](LICENSE).
