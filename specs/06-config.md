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

## managed_proof_max_attempts / managed_proof_retry_backoff_secs / managed_proof_probe_timeout_secs

Tune the managed-capability proof (network/SSH/writable-root) retry policy so a
transient probe failure (e.g. a brief network blip) self-heals instead of
permanently disabling dispatch.

- `managed_proof_max_attempts` — total proof attempts before the dispatch gate
  commits to `Failed`. `1` disables retry (legacy behavior). Default `3`.
- `managed_proof_retry_backoff_secs` — base back-off between retries; the delay
  grows exponentially from this value and is capped internally. Default `2`.
- `managed_proof_probe_timeout_secs` — child network/writable-root probe timeout.
  Raise it for slow-network environments. `0` falls back to the default. Default `45`.

Frontmatter and global config share these field names. Managed-proof resolution
chain (each field independently): `frontmatter > config > built-in default`.
Between retries the gate stays `Pending` (gated but recoverable); operator
`session clear` / `session interrupt-clear` / stop remain gate-exempt regardless.

`claude_args` and `AGENT_DOC_CLAUDE_ARGS` are ignored when the active harness is not Claude. `codex_args` is ignored when the active harness is not Codex. `opencode_args` is ignored when the active harness is not OpenCode.

## Project Config

Location: `.agent-doc/config.toml` (relative to project root).

Fields:
- `tmux_session` — the tmux session name bound to this project.
- `agent_doc_auto_compact = <line-threshold>` — explicit opt-in for automatic compaction/reload policies. Session-accretion warnings, repeated no-op closeouts, and Claude skill auto-update must not compact by default when this setting is absent.
- `ssh.profiles.<name>.targets = ["alias-or-host", ...]` — named SSH target groups for ops docs.
- `ssh.docs."<relative/path.md>"` — per-document SSH defaults for known ops docs. Each entry may set `profile = "<name>"`, direct `targets = [...]`, or both. If an entry exists but resolves no targets, preflight/startup must fail closed.
- `documents.include = ["tasks/**/*.md", "plan.md", ...]` — project-relative globs whose matching `.md` files are treated as agent-doc session documents (opt-in gate). Globs support `*` (within a path segment), `?` (one char), and `**` (spans `/`, including zero segments).
- `documents.auto_session_for_all_md = <bool>` — legacy escape hatch restoring the old "every `.md` is a session" behavior. Default `false`.

**Auto-sync:** When the configured `tmux_session` is dead (session no longer exists), the route path falls back to `current_tmux_session()` and auto-updates `config.toml` with the new session name. This prevents stale config after session destruction.

## Opt-in Document Gate

A plain `.md` is **not** auto-converted into an agent-doc session. `route`, `run`, and `start` fail closed before injecting `agent_doc_session:` frontmatter unless the document opts in. The pure predicate `agent_doc_core::project_config::is_agent_doc_document(rel_path, content, config)` (FFI: `agent_doc_is_session_document(path)`) returns true when ANY of:

1. `documents.auto_session_for_all_md = true` (escape hatch).
2. Frontmatter carries any agent-doc-managed field — `Frontmatter::has_agent_doc_marker()` (e.g. `agent_doc_session`/`session`, `agent_doc_format`, `agent_doc_write`, `agent_doc_mode`, `agent_doc_stream`, `agent`, `resume`, model overrides, `*_args`, `branch`, `queue_active`, `prompt_presets`, `agent_doc_pipeline`). Existing sessions stay sessions.
3. The project-relative path matches a `documents.include` glob.

When none hold, the gate returns an error naming the opt-in paths (`agent-doc init <file>`, an `agent_doc_format:` field, a `[documents] include` glob, or the escape hatch) and **does not mutate the file**. A malformed frontmatter block bypasses the gate so its own contextual YAML parse error surfaces. `agent-doc init <file>` and `agent-doc claim <file>` remain explicit per-file opt-ins that scaffold. Editor plugins should call the FFI gate so **Run Agent Doc** / SubmitAction is hidden for non-opted-in `.md`.

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
5. **Append patch idempotence:** Editor-side component patch application must treat append-mode patches as replay-safe. Before appending a patch into `agent:exchange` or another append component, the native/shared patcher and editor fallbacks compare the normalized existing component body against the normalized patch body, ignoring transient `(HEAD)` response-heading markers and `agent:boundary` comments. If the patch body is already present, the plugin must leave the document unchanged instead of duplicating the exchange. JetBrains File Cache Conflict retries also revalidate against the current disk file and committed `HEAD`; `HEAD` is accepted as proof only when the disk file still matches it, so stale editor buffers are not left missing a committed response.
6. **Snapshot freshness cleanup:** On pending-patch pickup, if `.agent-doc/snapshots/<hash>.md` is newer than the patch file, the patch is stale and must be deleted without apply.
7. **Malformed patchback rejection:** Before socket or file IPC delivery, the CLI validates the parsed template patchback shape. If patch/replace markers are present outside code blocks but no closed patch blocks parse, the write fails closed and logs `template_patchback_malformed_rejected` instead of sending raw unmatched text that the plugin could append into `agent:exchange`.
8. **Forensic sidecar logging:** Socket IPC sends log `ipc_socket_attempt` with patch counts, synthesized IPC patch counts, unmatched lengths, baseline length, normalization target count, and unmatched marker count. Successful ack sidecar reads log `ipc_socket_ack_content` with sidecar and pre-write disk hashes/lengths before snapshot persistence.

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

CRDT sidecar state is versioned. Legacy states store the whole markdown document in a Yrs text root named `content`. The structured markdown AST overlay stores `agent_doc_overlay.schema_version = 1`, a `markdown` projection, and a `components` Yrs array whose entries are component maps (`name`, `attrs`, byte span, `items`). Item entries are maps (`id`, `text`, `raw`, byte span, `kind`, `struck`, `pinned`, `agent_pinned`); `kind` is itself a map with `type` and optional `checkbox`. Loading an old `content` state migrates by reparsing that markdown through the component overlay, while empty legacy state can use the current markdown file as a fallback.

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
