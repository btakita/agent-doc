> Extracted from SPEC.md — see [index](../SPEC.md)

# Agent Backend

## Trait

`fn send(prompt, session_id, fork, model) -> (text, session_id)`

## Resolution Order

1. CLI `--agent` flag
2. Frontmatter `agent` field
3. Config `default_agent`
4. Fallback: `"claude"`

## Claude Backend

Default: `claude -p --output-format json --permission-mode acceptEdits`. Session handling: `--resume {id}` or `--continue --fork-session`. Appends `--append-system-prompt` with document-mode instructions. Removes `CLAUDECODE` env var. Parses JSON: `result`, `session_id`, `is_error`.

## Custom Backends

Config overrides `command` and `args` for any agent name.
