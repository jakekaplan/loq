# loq

[![CI](https://github.com/jakekaplan/loq/actions/workflows/ci.yml/badge.svg)](https://github.com/jakekaplan/loq/actions/workflows/ci.yml)
[![codecov](https://codecov.io/gh/jakekaplan/loq/graph/badge.svg)](https://codecov.io/gh/jakekaplan/loq)
[![PyPI](https://img.shields.io/pypi/v/loq)](https://pypi.org/project/loq/)
[![Crates.io](https://img.shields.io/crates/v/loq)](https://crates.io/crates/loq)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)

An electric fence for LLMs (and humans too). Written in Rust,
`loq` enforces file line limits: fast, zero-config, and language agnostic.

## Why loq?
- 🔒 Hard limits to prevent oversized files and context rot
- 📏 One metric: line counts (`wc -l` style)
- 🧩 Works everywhere - no language-specific setup
- 🤖 Designed specifically with coding agents in mind
- 🦀 Lightning fast Rust core

## Getting Started

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
loq check                          # Check current directory (500 line default)
loq check src/ lib/                # Check specific paths
git diff --name-only | loq check - # Check files from stdin
```

## Output

Token-efficient default output:

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

Use `--output-format json` for machine-readable output:

```bash
loq check --output-format json
```

```json
{
  "version": "0.1.0",
  "violations": [
    {
      "path": "src/main.rs",
      "lines": 1427,
      "max_lines": 500,
      "rule": "default"
    }
  ],
  "skip_warnings": [],
  "walk_errors": [],
  "summary": {
    "files_checked": 42,
    "skipped": 0,
    "passed": 41,
    "violations": 1,
    "walk_errors": 0
  }
}
```

## Configuration

loq works zero-config. Run `loq init` to create a `loq.toml` file to customize:

```toml
# default, for files not matching any rule
default_max_lines = 500       

# skip .gitignore'd files
respect_gitignore = true      

# ignore files or paths
exclude = [".git/**", "**/generated/**", "*.lock"]

# Add fix_guidance to include project-specific instructions 
# with each violation when piping output to an LLM:
fix_guidance = "Split large files: helpers → src/utils/, types → src/types/"

# last match wins, ** matches any path
[[rules]]                   
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
are under the threshold, and ignores files that grew. Files cannot be
rebaselined to a higher limit unless you pass `--allow-growth`. Use
`--threshold 300` to set a custom limit.

Need to ship while files are still too big? Accept defeat creates or updates
exact-path rules for the files currently failing checks:

```bash
loq accept-defeat                # Use default buffer of 100 lines
loq accept-defeat src/legacy.rs  # Only update for one file
loq accept-defeat --buffer 50    # Add 50 lines above current size
```

## Add as a Pre-commit Hook

```yaml
repos:
  - repo: https://github.com/jakekaplan/loq
    rev: v0.1.0-alpha.5
    hooks:
      - id: loq
```

## Contributing

Contributions are welcome! See [CONTRIBUTING.md](CONTRIBUTING.md) for development setup and guidelines.

## License

This project is licensed under the [MIT License](LICENSE).
