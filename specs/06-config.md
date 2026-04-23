> Extracted from SPEC.md — see [index](../SPEC.md)

# Config

Location: `{XDG_CONFIG_HOME}/agent-doc/config.toml` (default `~/.config/agent-doc/config.toml`).

Fields: `default_agent`, `agent_args`, `claude_args`, `codex_args`, `[agents.{name}]` with `command`, `args`, `result_path` (reserved), `session_path` (reserved).

## agent_args

Additional CLI arguments passed to the active agent process when spawned by `agent-doc start`.
Space-separated string.

For both Claude and Codex, `agent_args` is the harness-neutral override and takes precedence over harness-specific aliases.

## claude_args

Additional CLI arguments passed to the `claude` process when spawned by `agent-doc start`.
Space-separated string. Claude-only compatibility alias for older documents/configs.

## codex_args

Additional CLI arguments passed to the `codex` process when spawned by `agent-doc start`.
Space-separated string. Codex-only alias for explicit Codex session configuration.

Claude sources, in precedence order (highest first):

1. **Frontmatter**: `agent_args: "--model sonnet"`, `claude_args: "--dangerously-skip-permissions"`, or `codex_args: "-s danger-full-access"` in the document's YAML frontmatter
2. **Global config**: `agent_args = "--model sonnet"`, `claude_args = "--dangerously-skip-permissions"`, or `codex_args = "-s danger-full-access"` in `~/.config/agent-doc/config.toml`
3. **Environment variable**: `AGENT_DOC_CLAUDE_ARGS="--dangerously-skip-permissions"`

Claude resolution chain: `frontmatter agent_args > frontmatter claude_args > config agent_args > config claude_args > AGENT_DOC_CLAUDE_ARGS`.

Codex resolution chain: `frontmatter agent_args > frontmatter codex_args > config agent_args > config codex_args`.

`claude_args` and `AGENT_DOC_CLAUDE_ARGS` are ignored when the active harness is not Claude. `codex_args` is ignored when the active harness is not Codex.

## Project Config

Location: `.agent-doc/config.toml` (relative to project root).

Fields: `tmux_session` — the tmux session name bound to this project.

**Auto-sync:** When the configured `tmux_session` is dead (session no longer exists), the route path falls back to `current_tmux_session()` and auto-updates `config.toml` with the new session name. This prevents stale config after session destruction.

## Socket IPC

Socket-based IPC via Unix domain sockets (`.agent-doc/ipc.sock`) is the primary IPC transport. The editor plugin starts a listener via `agent_doc_start_ipc_listener()` FFI call on project open. The CLI sender connects, sends NDJSON messages, and waits for ack.

**Protocol:** Newline-delimited JSON (NDJSON). Message types:
- `{"type": "patch", "file": "...", "patches": [...], ...}` — apply component patches
- `{"type": "reposition", "file": "..."}` — reposition boundary marker
- `{"type": "vcs_refresh"}` — trigger VCS/VFS refresh

**Fallback:** If socket is unavailable (no listener), falls back to file-based IPC (JSON patch files in `.agent-doc/patches/`).

## IPC Write Verification

After the IDE plugin consumes an IPC patch file:
1. **File-change check:** If the document file is unchanged on disk, the plugin failed to apply — falls back to disk write.
2. **Content verification:** If the document changed but none of the patch content appears in the result, the plugin partially failed — falls back to disk write.
3. **Force-disk cleanup:** When `--force-disk` is set, any pending IPC patch files are deleted before disk write to prevent the plugin from applying stale patches (double-write prevention).

## Sync Layout Authority

`sync_after_claim()` uses editor-provided `col_args` when available (authoritative layout from the IDE plugin). Only falls back to registry-based file discovery when no `col_args` given. This prevents stale registry entries from creating incorrect multi-pane layouts.

## Document State Model (4 States)

A document has four concurrent representations during a write cycle:

| State | Location | Owner | Purpose |
|-------|----------|-------|---------|
| **Snapshot** | `.agent-doc/snapshots/<hash>.md` | Binary | Last committed agent state. Used by `diff::compute()` to detect user changes since last response. |
| **Baseline** | `.agent-doc/baselines/<hash>.md` | Binary (preflight) | Document at start of response generation. Common ancestor for 3-way/CRDT merge. Saved by preflight after commit (step 2b). |
| **File on disk** | The document file | Editor (auto-save) | Last editor save. Lags behind the editor buffer. Used by non-IPC write paths. |
| **Editor buffer** | Editor memory | Editor (Document API) | Live content including unsaved edits. IPC writes target this via the Document API, preserving cursor position and undo history. |

**Consistency invariants:**
- After preflight step 2b: `baseline == snapshot` (minus boundary markers)
- After `agent-doc write`: `snapshot == baseline + response` (content_ours)
- After `agent-doc commit`: git HEAD contains `snapshot + (HEAD) marker`
- The editor buffer may diverge from all three persistent states (unsaved user edits)

**Staleness risk:** If the baseline is saved before preflight (the old SKILL.md approach), it becomes stale when commit repositions the boundary marker. The binary guard in `write.rs` detects this via component-aware comparison:
- Parses both snapshot and baseline into components (`component::parse`)
- Only checks **append-mode** components (exchange, findings) — these grow monotonically
- Skips **replace-mode** components (status, pending) — user-editable, expected to diverge
- Falls back to prefix check for non-template (inline) documents
- When stale: re-applies patches to current file content instead of the stale baseline
