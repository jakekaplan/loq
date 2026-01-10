# fence

fence is a fast, minimal "electric fence" for file sizes. It keeps codebases LLM-context-friendly and human-maintainable by enforcing hard per-file line limits.

LLMs happily generate big files. Big files burn context, slow reviews, and become dumping grounds. fence is a hard constraint: you do not negotiate with a fence. You hit it and stop.

## Install

```bash
cargo install fence
```

## Quick start

```bash
# zero-config: uses built-in defaults
fence

# explicit check
fence check src/

# stdin list
git diff --cached --name-only | fence check -
```

## Output

Violations are structured and LLM-parseable:

```
error[max-lines]: src/utils.py: 512 lines (limit: 400, +112 over)
```

Summary:

```
10 files checked, 2 skipped, 5 passed, 2 errors, 1 warning (15ms)
```

## Config

Create a config with defaults:

```bash
fence init
```

Baseline a legacy repo (exempts current errors, blocks new ones):

```bash
fence init --baseline
```

Config discovery walks upward from each file’s directory and uses the nearest `.fence.toml`. Patterns are matched against paths relative to the config directory.

## Default `.fence.toml`

```toml
# fence: an "electric fence" that keeps files small for humans and LLMs.
# Counted lines are wc -l style (includes blanks/comments).

default_max_lines = 400

exclude = [
  ".git/**",
  "target/**",
  "node_modules/**",
  "vendor/**",
  "dist/**",
  "build/**",
  ".venv/**",
  "venv/**",
  "__pycache__/**",
  ".mypy_cache/**",
  ".pytest_cache/**",
  ".ruff_cache/**",
]

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
