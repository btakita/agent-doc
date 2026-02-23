# Configuration

## Config file

Location: `~/.config/agent-doc/config.toml`

```toml
default_agent = "claude"

[agents.claude]
command = "claude"
args = ["-p", "--output-format", "json"]
result_path = ".result"
session_path = ".session_id"

[agents.codex]
command = "codex"
args = ["--prompt"]
result_path = ".output"
session_path = ".id"
```

## Fields

| Field | Description |
|-------|-------------|
| `default_agent` | Agent backend used when not specified elsewhere |
| `[agents.NAME]` | Agent backend configuration |
| `command` | Executable name or path |
| `args` | Arguments passed before the prompt |
| `result_path` | JSON path to extract the response text |
| `session_path` | JSON path to extract the session ID |

## Resolution order

The agent backend is resolved in this order:

1. `--agent` CLI flag
2. `agent:` field in document frontmatter
3. `default_agent` in config
4. Fallback: `"claude"`

## Per-document overrides

Set `agent` and `model` in the document's YAML frontmatter:

```yaml
---
session: null
agent: codex
model: gpt-4
---
```

These override the config file for that specific document.
