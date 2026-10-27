# Configuration

## Config file

Location: `~/.config/agent-doc/config.toml`

```toml
default_agent = "claude"
tmux_bin = "/opt/tmux/bin/tmux"

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

[queue_icons]
# Strings may contain multiple characters; every field is optional.
pending = "📥"
reported = "☑️"
```

## Fields

| Field | Description |
|-------|-------------|
| `default_agent` | Agent backend used when not specified elsewhere |
| `tmux_bin` | Machine-wide tmux client; project `tmux_bin` overrides it |
| `[agents.NAME]` | Agent backend configuration |
| `command` | Executable name or path |
| `args` | Arguments passed before the prompt |
| `result_path` | JSON path to extract the response text |
| `session_path` | JSON path to extract the session ID |
| `[terminal].host` | Terminal presentation host: `auto`, `ide`, `external`, or `none` |
| `[terminal].command` | External-terminal command template; supports `{tmux_command}` |
| `[terminal].auto_start_tmux` | Allow creation of a missing tmux session (default `true`) |
| `[terminal].attach_command` | IDE attach template; supports `{session}` and `{tmux_command}` |
| `[queue_icons]` | Optional visible queue prefix strings; fields are `pending`, `claimed`, `running`, `reported`, `lost`, `reconciling`, `unknown`, `skipped`, `awaiting_operator`, `operator_action`, `prioritized`, and `agent_prioritized` |

## Resolution order

The agent backend is resolved in this order:

1. `--agent` CLI flag
2. `agent:` field in document frontmatter
3. `agent:` field in the nearest `.agent-doc/project.md` frontmatter
4. `default_agent` in the project's `.agent-doc/config.toml`
5. `default_agent` in the global config
6. Fallback: `"claude"`

## Project-wide frontmatter defaults

Any document frontmatter field can be supplied as a project default in
`.agent-doc/project.md`:

```yaml
---
agent: claude
model: sonnet
agent_doc_clear_threshold: 60
env:
  PROJECT_MODE: development
---

# Project defaults
```

Agent Doc reads this file dynamically. Existing open sessions pick up changes
to both `.agent-doc/project.md` and `.agent-doc/config.toml` after the project
configuration change is observed; inherited values are never copied into the
session document. A document declaration wins by top-level key, including an
explicit empty list or map. Nested project roots override ancestor project
defaults. `prompt_presets`/`presets` retain their per-preset merge behavior so a
document can override one shared preset without replacing the rest.

## Per-document overrides

Set `agent` and `model` in the document's YAML frontmatter:

```yaml
---
session: null
agent: opencode
opencode_model: zai/glm-5
---
```

These override both project-wide frontmatter and the config file for that
specific document.

`terminal_host: auto|ide|external|none` may also be set in document
frontmatter. Its precedence is document, project, global, then `auto`. The other
terminal fields use project then global precedence. Terminal configuration does
not contain a session name: use project `tmux_session` / `agent-doc session set`.
The tmux executable uses project `tmux_bin`, then global `tmux_bin`, then `PATH`;
`agent-doc env --json` reports the selected binary and its source.
See the [Coder workspace terminal runbook](../../runbooks/coder-workspace.md) for
remote IDE setup and headless failure behavior.

To dogfood Agent Doc itself, opt a document into typed, deduplicated findings:

```yaml
---
dogfood:
  path: tasks/agent-doc/agent-doc-bugs.md
  auto: true
  prompt: >-
    File a GitHub issue with the diagnostic instead of changing code.
---
```

`path` is project-relative and defaults to the current document. `auto` defaults
to false; when true, only generated stable-keyed findings are queued. The same
typed value may be set in `.agent-doc/project.md`, while session frontmatter
wins. `dogfood: false` disables inheritance and absence everywhere is off.
`prompt` optionally replaces the default “diagnose and fix” action while keeping
the stable issue metadata and retained-write safety guidance. Blank prompts use
the default action, and map fields do not merge across precedence layers.
`agent-doc dogfood policy <FILE> --json` shows the effective destination and
auto-queue setting plus any custom prompt after applying that precedence.
Watchers use this typed query instead of parsing either document themselves.
Set a document-local policy without editing live frontmatter directly:

```sh
agent-doc dogfood set tasks/session.md
agent-doc dogfood set tasks/session.md \
  --path tasks/agent-doc/agent-doc-bugs.md --auto true \
  --prompt 'File a GitHub issue with the diagnostic instead of changing code.'
```

The first form targets the current document with `auto: false`. The second path
is relative to the project root. The command changes only the public `dogfood:`
frontmatter field through editor-aware document authority and preserves all
other frontmatter and body bytes.
Invariant diagnosis and uniquely-safe projection repair remain on regardless.

To opt a document into retained-write-owned terminal convergence:

```yaml
---
agent_doc_per_component_convergence: true
---
```

The same experiment can be enabled project-wide in `.agent-doc/config.toml`
with `agent_doc_per_component_convergence = true`. It is off by default.
When enabled, an operator edit in an unowned component such as `agent:run`
does not block an agent response that owns only `agent:exchange`. Invalid or
legacy ownership evidence falls back to whole-document equality.

## Environment Variables

### Runtime Tuning

| Variable | Default | Description |
|----------|---------|-------------|
| `AGENT_DOC_LOG` | — | Structured log filter (e.g. `debug`, `agent_doc::preflight=debug`). Logs to `.agent-doc/logs/debug.log.<date>` of the nearest project root within `AGENT_DOC_ROOT_CEILING_DIRECTORIES`; disabled when there is none |
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
| `default_agent` | Default backend for this project (for example, `"codex"`); overrides the global default, while document frontmatter and `--agent` still win |
| `tmux_session` | Tmux session name bound to this project |
| `tmux_sessions` | Optional allow-list for multi-session projects (one editor, several tmux sessions); documents bind with `tmux_session:` frontmatter. Empty keeps single-session behavior. See `runbooks/multi-tmux-sessions.md` |
| `[terminal]` | Project terminal policy; the same fields as global `[terminal]` except session naming remains top-level `tmux_session` |
| `[commit] push` | Optional post-closeout publication policy. `"off"` (the default) leaves commits local; `"ff-only"` pushes `HEAD` to the current branch's configured upstream without force. Rejections are warnings and do not invalidate the committed cycle. |
| `agent_doc_auto_compact` | Line threshold for automatic compaction opt-in |
| `agent_doc_supervisor_stderr_log` | Supervisor stderr log path. Relative paths resolve from the project root; absolute paths are used as written. Defaults to `.agent-doc/logs/supervisor-stderr.log` |
| `documents.include` | Project-relative globs for session document opt-in |
| `documents.auto_session_for_all_md` | Legacy escape hatch (default `false`) |

## Project-wide prompt presets

Project-wide prompt presets do not go in `.agent-doc/config.toml`. After
initializing the project with `agent-doc init`, create or edit
`.agent-doc/project.md` and define presets in its YAML frontmatter:

```markdown
---
presets:
  "#review": Review the change and run focused tests.
  "#release":
    prompt: Prepare and publish the release.
    runbook: runbooks/release.md
---

# Agent Doc project defaults
```

Quote names that begin with `#`; otherwise YAML treats the rest of the key as
a comment. `prompt_presets:` is accepted as an alias for `presets:`.

Invoke a preset by adding its name to a session prompt or queue, for example
`#review` in `agent:exchange` or `- #review` in `agent:run`. Every ancestor
`.agent-doc/project.md` supplies live defaults within the configured root
ceiling. A nested project's values overlay its parent's values, and a session
document can override either by defining the same exact name in its own
frontmatter. Relative `runbook` paths resolve from the project root and are
loaded only when the preset is invoked.

See [Document Format](../guide/document-format.md#prompt-presets) for the full
inheritance and masking rules.

To publish each newly created closeout commit to the current branch's configured
upstream, opt in explicitly:

```toml
[commit]
push = "ff-only"
```

The push uses an explicit `HEAD:<upstream-branch>` refspec, disables force,
mirror, tag, and recursive-submodule modes, and runs only after the local
closeout transaction succeeds. A missing upstream, non-fast-forward rejection,
or transport failure is logged as an advisory warning; the local committed
cycle remains valid.

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
