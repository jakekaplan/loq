# loq

An electric fence for LLM-generated code.

LLMs are great at generating code, but sometimes they go off the rails. You can tell an LLM what to do, but the only way to **guarantee** it listens is with feedback loops and hard constraints. loq provides that constraint: a fast, dead-simple way to enforce file size limits.

## Why file size matters

Big files mean more tokens. More tokens mean:

- **Slower responses** - LLMs take longer to process what they don't need
- **Higher costs** - You pay per token
- **Worse output** - Context windows fill up, important details get lost
- **Code rot** - Large files become dumping grounds that humans avoid refactoring

loq stops the sprawl before it starts.

## Install

```bash
# With uv (recommended)
uv tool install loq

# With pip
pip install loq

# With cargo
cargo install loq
```

## Quick start

```bash
# Zero-config check (500 line default)
loq check

# Check specific paths
loq check src/ lib/

# Check staged files in pre-commit
git diff --cached --name-only | loq check -
```

## LLM-first design

loq is built for AI coding workflows. Output is token-efficient:

```
✖  1,427 > 500   src/components/Dashboard.tsx
✖    892 > 500   src/utils/helpers.py
2 violations (14ms)
```

No walls of text. No redundant explanations. Just what you need to fix.

Use `-v` for additional context when debugging:

```
✖  1,427 > 500   src/components/Dashboard.tsx
                  ├─ rule:   max-lines=500 severity=error (match: **/*.tsx)
                  └─ config: loq.toml
```

## Pre-commit integration

```yaml
# .pre-commit-config.yaml
repos:
  - repo: local
    hooks:
      - id: loq
        name: loq check
        entry: loq check
        language: system
        pass_filenames: false
```

Now your LLM gets immediate feedback when it generates oversized files.

## Baseline legacy repos

Have a codebase with existing large files? Lock them at their current size:

```bash
loq init --baseline
```

This generates rules that allow existing files to stay at their current line count, but any growth triggers an error. Ratchet down over time.

## Config

Create a config with sensible defaults:

```bash
loq init
```

Example `loq.toml`:

```toml
default_max_lines = 500
respect_gitignore = true
exclude = ["**/generated/**", "**/vendor/**"]

# Last match wins
[[rules]]
path = "**/*.tsx"
max_lines = 300
severity = "warning"   # warn but don't fail

[[rules]]
path = "tests/**/*"
max_lines = 600        # tests can be longer
```

### Config options

| Option | Default | Description |
|--------|---------|-------------|
| `default_max_lines` | `500` | Limit for files not matching any rule |
| `respect_gitignore` | `true` | Skip files matched by `.gitignore` |
| `exclude` | `[]` | Glob patterns to skip |
| `rules` | `[]` | Path-specific overrides (last match wins) |

### Rule options

| Option | Default | Description |
|--------|---------|-------------|
| `path` | required | Glob pattern(s) to match |
| `max_lines` | required | Line limit for matched files |
| `severity` | `"error"` | `"error"` (fails check) or `"warning"` (reports only) |

Config discovery walks upward from each file and uses the nearest `loq.toml`.

## CLI reference

```bash
loq check [PATHS...]   # Check files (default: current directory)
loq init               # Create loq.toml with defaults
loq init --baseline    # Create loq.toml locking current violations

# Flags
-q, --quiet            # Suppress summary
--silent               # Suppress all output
-v, --verbose          # Show rule/config details
--config <PATH>        # Use specific config file
```

## License

MIT.
