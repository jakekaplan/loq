# loq

An electric fence for LLM-generated code.

LLMs are great at generating code, but sometimes they go off the rails. You can tell an LLM what to do, but the only way to **guarantee** it listens is with feedback loops and hard constraints. loq provides that constraint: a fast, dead-simple way to enforce file size limits.

## Why file size matters

Big files mean more tokens. More tokens mean:

- **Slower responses** - LLMs take longer to process what they don't need
- **Higher costs** - you pay per token
- **Context rot** - large files become dumping grounds that overwhelm both LLMs and humans

loq stops the sprawl before it starts.

## Why loq?

Linters like Ruff and ESLint check correctness. loq checks size.

It does one thing: enforce line counts (`wc -l` style). No parsers, no plugins, language agnostic. One tool for your entire polyglot monorepo.

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
loq                   # Check current directory (zero-config, 500 line default)
loq check src/ lib/   # Check specific paths
loq init              # Create loq.toml with defaults
loq init --baseline   # Lock existing files at current size
```

### Pre-commit

```yaml
# .pre-commit-config.yaml
repos:
  - repo: local
    hooks:
      - id: loq
        name: loq
        entry: loq
        language: system
        pass_filenames: false
```

### LLM-friendly output

Output is designed to be token-efficient:

```
✖  1,427 > 500   src/components/Dashboard.tsx
✖    892 > 500   src/utils/helpers.py
2 violations (14ms)
```

Use `loq -v` for additional context when debugging:

```
✖  1,427 > 500   src/components/Dashboard.tsx
                  └─ rule: max-lines=500 severity=error (match: **/*.tsx)
```

## Configuration

loq works out of the box with sensible defaults. Create a config file to customize:

```bash
loq init
```

### Example

```toml
default_max_lines = 500
respect_gitignore = true
exclude = ["**/generated/**", "**/vendor/**"]

# Last match wins
[[rules]]
path = "**/*.tsx"
max_lines = 300
severity = "warning"

[[rules]]
path = "tests/**/*"
max_lines = 600
```

### Options

| Option | Default | Description |
|--------|---------|-------------|
| `default_max_lines` | `500` | Limit for files not matching any rule |
| `respect_gitignore` | `true` | Skip files matched by `.gitignore` |
| `exclude` | `[]` | Glob patterns to skip |
| `rules` | `[]` | Path-specific overrides |

### Rule options

| Option | Default | Description |
|--------|---------|-------------|
| `path` | required | Glob pattern(s) to match |
| `max_lines` | required | Line limit for matched files |
| `severity` | `"error"` | `"error"` or `"warning"` |

### Baseline

Have a codebase with existing large files? Lock them at their current size:

```bash
loq init --baseline
```

This generates rules that allow existing files to stay at their current line count, but any growth triggers an error. Ratchet down over time.

## Contributing

Contributions are welcome! See [CONTRIBUTING.md](CONTRIBUTING.md) for development setup and guidelines.

## License

This project is licensed under the [MIT License](LICENSE).
