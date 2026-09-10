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

## Grok Build

Install and sign in to [Grok Build](https://docs.x.ai/build/overview), then run
`agent-doc skill install --harness grok` in your project. This installs the
shared skill and registers the project `agent-doc` MCP server. Enable/trust that
server in Grok if prompted; existing server configuration is preserved.

Set `agent: grok` in document frontmatter (`grok-build` is also accepted).
`agent-doc route notes.md` opens the managed interactive session;
`agent-doc run notes.md --agent grok` uses the headless backend. Both resume
only the document's recorded session ID. A first headless run starts fresh.
Optional `agent_args` configure the interactive CLI; `[agents.grok]` overrides
the headless executable and base arguments. Explicit model names are passed
through; `[model.tiers.grok]` can override tier mappings.

The backend uses Grok's `--prompt-file ... --output-format json`, publishes only
`text` after `stopReason: end_turn`, and retains `sessionId` for resume. It does
not publish thoughts, truncated output, or tool events. Native token streaming
is not advertised. No Heavy-plan model is selected automatically.

Grok's passive Stop hooks cannot steer another turn. The installed skill uses
MCP admission/finalize and follows committed queue continuation in the same
turn. Pane dispatch uses the observed boxed composer and active cancel/stop
indicators; drafts and unrecognized UI remain protected. Tested with Grok Build
1.0.24. See the [CLI reference](https://docs.x.ai/build/cli/reference) and
[hook contract](https://docs.x.ai/build/features/hooks).

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
