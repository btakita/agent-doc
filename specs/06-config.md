> Extracted from SPEC.md — see [index](../SPEC.md)

# Config

Location: `{XDG_CONFIG_HOME}/agent-doc/config.toml` (default `~/.config/agent-doc/config.toml`).

Fields: `default_agent`, `agent_args`, `claude_args`, `codex_args`, `opencode_args`, `codex_network_access`, `[agents.{name}]` with `command`, `args`, `result_path` (reserved), `session_path` (reserved).

## agent_args

Additional CLI arguments passed to the active agent process when spawned by `agent-doc start`.
Space-separated string.

For Claude, Codex, and OpenCode, `agent_args` is the harness-neutral override and takes precedence over harness-specific aliases. When the active Claude/Codex document lives in a git submodule, agent-doc supplements the resolved args with `--add-dir` entries for the superproject working tree plus any external git metadata directories needed outside the submodule root, including nested child submodule gitdirs under `.git/modules/...`. That keeps submodule-rooted sessions able to patch parent-repo docs and complete the normal git lifecycle even when the task commits inside a nested submodule. Both Claude Code and Codex `exec` accept `--add-dir`; however, `codex exec resume` does not — the Codex backend strips `--add-dir` entries from resume args since the resumed session inherits writable roots from the original `exec`.

## claude_args

Additional CLI arguments passed to the `claude` process when spawned by `agent-doc start`.
Space-separated string. Claude-only compatibility alias for older documents/configs.

## codex_args

Additional CLI arguments passed to the `codex` process when spawned by `agent-doc start`.
Space-separated string. Codex-only alias for explicit Codex session configuration.

## opencode_args

Additional CLI arguments passed to the `opencode` process when spawned by `agent-doc start`.
Space-separated string. OpenCode-only alias for explicit OpenCode session configuration.

## codex_network_access

Explicit Codex network policy for agent-doc-launched sessions.

Values:
- `inherit` (default) — keep the launcher's ambient `CODEX_SANDBOX_NETWORK_DISABLED` setting
- `enabled` — remove `CODEX_SANDBOX_NETWORK_DISABLED` from the child env
- `disabled` — force `CODEX_SANDBOX_NETWORK_DISABLED=1` in the child env

Frontmatter and global config share the same field name: `codex_network_access`.

Claude sources, in precedence order (highest first):

1. **Frontmatter**: `agent_args: "--model sonnet"`, `claude_args: "--dangerously-skip-permissions"`, `codex_args: "-s danger-full-access"`, `opencode_args: "--dangerously-skip-permissions"`, or `codex_network_access: enabled` in the document's YAML frontmatter
2. **Global config**: `agent_args = "--model sonnet"`, `claude_args = "--dangerously-skip-permissions"`, `codex_args = "-s danger-full-access"`, `opencode_args = "--dangerously-skip-permissions"`, or `codex_network_access = "enabled"` in `~/.config/agent-doc/config.toml`
3. **Environment variable**: `AGENT_DOC_CLAUDE_ARGS="--dangerously-skip-permissions"`

Claude resolution chain: `frontmatter agent_args > frontmatter claude_args > config agent_args > config claude_args > AGENT_DOC_CLAUDE_ARGS`.

Codex resolution chain: `frontmatter agent_args > frontmatter codex_args > config agent_args > config codex_args`.

OpenCode resolution chain: `frontmatter agent_args > frontmatter opencode_args > config agent_args > config opencode_args`.

Codex network resolution chain: `frontmatter codex_network_access > config codex_network_access > inherit`.

`claude_args` and `AGENT_DOC_CLAUDE_ARGS` are ignored when the active harness is not Claude. `codex_args` is ignored when the active harness is not Codex. `opencode_args` is ignored when the active harness is not OpenCode.

## Project Config

Location: `.agent-doc/config.toml` (relative to project root).

Fields:
- `tmux_session` — the tmux session name bound to this project.
- `ssh.profiles.<name>.targets = ["alias-or-host", ...]` — named SSH target groups for ops docs.
- `ssh.docs."<relative/path.md>"` — per-document SSH defaults for known ops docs. Each entry may set `profile = "<name>"`, direct `targets = [...]`, or both. If an entry exists but resolves no targets, preflight/startup must fail closed.

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
4. **Claimed timeout cleanup:** If the CLI already completed a local IPC-timeout closeout, it writes `.agent-doc/claimed-patches/<patch_id>`. Plugins must treat that sentinel as a durable per-patch skip signal and delete the stale patch file instead of replaying it into the editor buffer; the sentinel must survive repeated watcher scans so multiple editor consumers cannot race a stale patch into a duplicate external edit.
5. **Snapshot freshness cleanup:** On pending-patch pickup, if `.agent-doc/snapshots/<hash>.md` is newer than the patch file, the patch is stale and must be deleted without apply.

## Sync Layout Authority

`sync_after_claim()` uses editor-provided `col_args` when available (authoritative layout from the IDE plugin). Only falls back to registry-based file discovery when no `col_args` given. This prevents stale registry entries from creating incorrect multi-pane layouts.
When that helper runs under an injected tmux handle (for example isolated route verification), the follow-up sync must stay on that same tmux server instead of falling back to the process-default server. Otherwise verification can accidentally stash panes in the operator's live `agent-doc` window while trying to reconcile a test-only layout.

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
- After `agent-doc commit`: git HEAD and the on-disk snapshot converge to the same clean boundary shape (no `(HEAD)` markers). The working tree and editor buffer preserve `(HEAD)` annotations on response headings
- The editor buffer may diverge from all three persistent states (unsaved user edits)

**Staleness risk:** If the baseline is saved before preflight (the old SKILL.md approach), it becomes stale when commit repositions the boundary marker. The binary guard in `write.rs` detects this via component-aware comparison:
- Parses both snapshot and baseline into components (`component::parse`)
- Only checks **append-mode** components (exchange, findings) — these grow monotonically
- Skips **replace-mode** components (status, pending) — user-editable, expected to diverge
- Falls back to prefix check for non-template (inline) documents
- When stale: re-applies patches to current file content instead of the stale baseline
