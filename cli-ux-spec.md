# loq CLI UX Spec

Summary of planned UX improvements based on design review.

## Output Formatting

- **Violation spacing**: Change `(+773 over limit)` from tab-indented to 1 space indent
- **Blank lines between violations**: Keep for readability
- **Unicode symbols**: Auto-detect terminal capability, fallback to ASCII (`x`, `!`, `.`) when needed
- **Colors**: Auto-detect TTY, disable for pipes
- **Success output**: Show stats summary (files checked, time taken)
- **Timing**: Keep `Time: 12ms` display

## Flag Changes

- **Merge `--quiet` and `--silent`**: Combine into single `--quiet` flag
- **No `--strict` flag**: Config handles severity via rules
- **No shell completions**: Not a priority

## Help & Discoverability

- **Help style**: Purely functional, no philosophy
- **Examples**: 1-2 key examples inline in `--help`
- **Flag grouping**: Logical grouping (output control, config, etc.) not alphabetical
- **Core message**: Focus on "how to run it" for first-time users
- **Typo handling**: Suggest closest command ("Did you mean 'check'?")
- **No aliases**: Explicit command names only (`init` not `i`)

## Behavior

- **Zero-config**: `loq` with no args runs check with defaults (500 line limit)
- **Opinionation**: Flexible but guided - good defaults with clear escape hatches
- **Violations**: Report only, no fix suggestions
- **Progressive disclosure**: Minimal by default, detail via `--verbose`
- **No JSON output**: Human-readable is sufficient
- **No progress indicators**: Just show results when done
- **No dry-run for init**: Not needed

## Error Handling

- **Invalid stdin paths**: Warn but continue processing valid ones
- **Invalid CLI paths**: Warn and continue (don't fail fast)
- **Config errors**: Point to exact line:col, no fix suggestions or doc links
- **`loq init` on existing config**: Refuse and warn (no overwrite)

## Init Command

- **`--baseline` output**: Just show count ("Exempted 5 files"), not the full list

## Stream Handling

- Follow common linter patterns (like ruff) for stdout/stderr

## Inspiration

- **Model after ruff**: Fast, clean output, good UX
