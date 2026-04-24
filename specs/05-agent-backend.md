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

## Prompt Contract

For resumed document turns, the prompt must include:

1. The unified diff since the last run
2. The full current document
3. An ordered `required response targets` section extracted from added user request blocks in the diff

That section is oldest-first. It exists so the agent does not anchor only on the newest visible question when the changed exchange tail contains multiple unresolved prompts. A turn is not complete until each listed target is answered or explicitly grouped into one response.

## Custom Backends

Config overrides `command` and `args` for any agent name.
