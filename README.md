# loq

[![CI](https://github.com/jakekaplan/loq/actions/workflows/ci.yml/badge.svg)](https://github.com/jakekaplan/loq/actions/workflows/ci.yml)
[![codecov](https://codecov.io/gh/jakekaplan/loq/graph/badge.svg)](https://codecov.io/gh/jakekaplan/loq)
[![PyPI](https://img.shields.io/pypi/v/loq)](https://pypi.org/project/loq/)
[![Crates.io](https://img.shields.io/crates/v/loq)](https://crates.io/crates/loq)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)

An electric fence for LLMs (and humans too). Written in Rust,
`loq` enforces file line limits: fast, works out of the box, and language agnostic.

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
# Check current directory for violations (default: 500 lines)
loq check           

# Check specific paths               
loq check src/ lib/     
     
# Check files from stdin      
git diff --name-only | loq check - 
```

### Managing legacy files

```bash
# Creates, updates or removes exact-path rules
# to match the current state of your files.
loq baseline

# Creates or updates exact-path rules for current violations
loq relax
loq relax src/legacy.rs   # specific file
loq relax --extra 50      # custom buffer

# Ratchets down or removes existing exact-path rules
# as your file sizes become compliant over time
loq tighten
```

All three commands manage exact-path rules in `loq.toml`. `baseline` and
`relax` can add new rules; `tighten` only updates or removes existing ones.

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

# Last match wins
# * stays within a path segment
# ** matches across directories
[[rules]]
path = "**/*.tsx"
max_lines = 300
```

## Output options

```bash
# Detailed output
loq check -v

# JSON format
loq check --output-format json
```

## Add as a Pre-commit Hook

```yaml
repos:
  - repo: https://github.com/jakekaplan/loq
    rev: v0.1.0-alpha.7
    hooks:
      - id: loq
```

## Contributing

Contributions are welcome! See [CONTRIBUTING.md](CONTRIBUTING.md) for development setup and guidelines.

## License

This project is licensed under the [MIT License](LICENSE).
