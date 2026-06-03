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

[agents.opencode]
command = "opencode"
args = ["run"]
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
agent: opencode
opencode_model: zai/glm-5
---
```

These override the config file for that specific document.

## Environment Variables

### Runtime Tuning

| Variable | Default | Description |
|----------|---------|-------------|
| `AGENT_DOC_LOG` | — | Structured log filter (e.g. `debug`, `agent_doc::preflight=debug`) |
| `AGENT_DOC_RUN_AGENT_TIMEOUT_SECS` | `1800` | Max agent run time before timeout (30 min) |
| `AGENT_DOC_RUN_HEARTBEAT_SECS` | `30` | Run heartbeat interval |
| `AGENT_DOC_QUEUE_MAX_ITERATIONS_HARD_CAP` | `50` | Hard cap on auto-loop queue iterations |
| `AGENT_DOC_TSIFT_BIN` | — | Override tsift binary path |
| `AGENT_DOC_TSIFT_GRAPH_TIMEOUT_SECS` | — | Tsift graph query timeout |

### Harness Detection

These are read by `detect_harness()` to identify the active agent harness:

| Variable | Harness |
|----------|---------|
| `CLAUDE_CODE_SESSION` / `CLAUDE_CODE` / `CLAUDECODE` | Claude Code |
| `CODEX_SESSION` / `CODEX_THREAD_ID` / `CODEX_CLI` / `CODEX` | Codex |
| `OPENCODE_CLIENT` / `OPENCODE` | OpenCode |

### Harness Arg Overrides

| Variable | Description |
|----------|-------------|
| `AGENT_DOC_CLAUDE_ARGS` | Claude CLI args (lowest precedence, below frontmatter + config) |

### Vision

| Variable | Description |
|----------|-------------|
| `AGENT_DOC_VISION_PROVIDER` | Vision provider override (e.g. `openai`) |
| `AGENT_DOC_VISION_API_KEY` | Vision API key |
| `AGENT_DOC_VISION_MODEL` | Vision model override |
| `AGENT_DOC_VISION_ENDPOINT` | Vision endpoint override |

### Testing / Debug

| Variable | Description |
|----------|-------------|
| `AGENT_DOC_NO_AUTOSTART` | Prevent auto-start of new agent panes |
| `AGENT_DOC_ROUTE_BIN` | Override agent-doc binary for route dispatch |
| `AGENT_DOC_DEBUG_FILTER` | Debug filter for supervisor start |
| `AGENT_DOC_DEBUG_STDIN` | Debug stdin for supervisor start |
| `AGENT_DOC_ALLOW_REPLACE_PENDING` | Allow replacing pending items |
| `AGENT_DOC_ALLOW_PATCH_PENDING` | Legacy alias for above (deprecated) |
| `AGENT_DOC_HARNESS_PROMPT` | Override harness prompt detection |

### Probe Markers

Set by child processes to confirm capability proofs:

| Variable | Description |
|----------|-------------|
| `AGENT_DOC_NETWORK_PROBE_OK` | Codex child network probe success |
| `AGENT_DOC_WRITABLE_ROOT_PROBE_OK` | Codex child writable root probe |
| `AGENT_DOC_OPENCODE_SSH_PROBE_OK` | OpenCode child SSH probe |

## Project Config

Location: `.agent-doc/config.toml` (relative to project root).

| Field | Description |
|-------|-------------|
| `tmux_session` | Tmux session name bound to this project |
| `agent_doc_auto_compact` | Line threshold for automatic compaction opt-in |
| `documents.include` | Project-relative globs for session document opt-in |
| `documents.auto_session_for_all_md` | Legacy escape hatch (default `false`) |

### SSH Config

```toml
[ssh.profiles.production]
targets = ["host1", "host2"]

[ssh.docs."ops/deploy.md"]
profile = "production"
```

### Component Config

Inline component attributes override defaults:

```markdown
<!-- agent:exchange patch=append max_lines=50 -->
```
