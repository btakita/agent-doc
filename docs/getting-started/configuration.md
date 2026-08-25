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

[terminal]
host = "auto"
auto_start_tmux = true
# command = "wezterm start -- {tmux_command}"
# attach_command = "tmux attach-session -t {session}"
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
| `[terminal].host` | Terminal presentation host: `auto`, `ide`, `external`, or `none` |
| `[terminal].command` | External-terminal command template; supports `{tmux_command}` |
| `[terminal].auto_start_tmux` | Allow creation of a missing tmux session (default `true`) |
| `[terminal].attach_command` | IDE attach template; supports `{session}` and `{tmux_command}` |

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

`terminal_host: auto|ide|external|none` may also be set in document
frontmatter. Its precedence is document, project, global, then `auto`. The other
terminal fields use project then global precedence. Terminal configuration does
not contain a session name: use project `tmux_session` / `agent-doc session set`.
See the [Coder workspace terminal runbook](../../runbooks/coder-workspace.md) for
remote IDE setup and headless failure behavior.

To dogfood Agent Doc itself, opt a document into actionable failure prompts:

```yaml
---
agent_doc_dogfood: true
---
```

When an Agent Doc command for that document fails to reach a successful
terminal boundary, the error includes a stable `ACTIONABLE_AGENT_DOC_FIX_PROMPT`
issue key and asks the active agent to fix the underlying product defect.
`agent_doc_dogfood: false` disables legacy path-inferred dogfood behavior.

To opt a document into retained-write-owned terminal convergence:

```yaml
---
agent_doc_per_component_convergence: true
---
```

The same experiment can be enabled project-wide in `.agent-doc/config.toml`
with `agent_doc_per_component_convergence = true`. It is off by default.
When enabled, an operator edit in an unowned component such as `agent:queue`
does not block an agent response that owns only `agent:exchange`. Invalid or
legacy ownership evidence falls back to whole-document equality.

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
| `[terminal]` | Project terminal policy; the same fields as global `[terminal]` except session naming remains top-level `tmux_session` |
| `agent_doc_auto_compact` | Line threshold for automatic compaction opt-in |
| `agent_doc_supervisor_stderr_log` | Supervisor stderr log path. Relative paths resolve from the project root; absolute paths are used as written. Defaults to `.agent-doc/logs/supervisor-stderr.log` |
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
