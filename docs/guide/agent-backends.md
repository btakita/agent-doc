# Agent Backends

agent-doc has an agent-agnostic core. Only the "send prompt, get response" step varies per backend.

## Claude (default)

The built-in Claude backend runs:

```
claude -p --output-format json --permission-mode acceptEdits
```

Session handling:
- First run: `--continue --fork-session` (inherits context from the most recent session)
- Subsequent runs: `--resume <session_id>` (continues the specific session)

The backend removes the `CLAUDECODE` environment variable to prevent nested session conflicts.

## Custom backends

Configure in `~/.config/agent-doc/config.toml`:

```toml
[agents.codex]
command = "codex"
args = ["--prompt"]
result_path = ".output"
session_path = ".id"
```

| Field | Description |
|-------|-------------|
| `command` | Executable name or path |
| `args` | Arguments passed before the prompt |
| `result_path` | JSON path to extract the response text from output |
| `session_path` | JSON path to extract the session ID from output |

## Backend contract

Each agent backend implements: take a prompt string, return `(response_text, session_id)`.

The prompt includes the diff and full document. The backend handles CLI invocation, JSON parsing, and session flags.

## Per-document override

Set `agent:` in the document's YAML frontmatter to use a specific backend for that document:

```yaml
---
agent: codex
model: gpt-4
---
```

Or override per-invocation:

```sh
agent-doc run session.md --agent codex --model gpt-4
```
