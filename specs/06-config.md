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

## managed_proof

The managed Codex capability proof is off by default. Set
`managed_proof: true` in document frontmatter to opt in. Project/global config
cannot enable it implicitly. Once enabled, network access, required SSH targets,
or resolved `--add-dir` writable roots select the proof phases; with none of
those requirements the gate remains `NotRequired`.

OpenCode capability-proof selection is unchanged and does not depend on this
Codex opt-in.

## managed_proof_max_attempts / managed_proof_retry_backoff_secs / managed_proof_probe_timeout_secs

Tune an opted-in managed-capability proof (network/SSH/writable-root) retry policy so a
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
Codex probe children are ephemeral, skip project rule loading, force low
reasoning effort, and combine network plus writable-root checks into one child
when both are required.

`claude_args` and `AGENT_DOC_CLAUDE_ARGS` are ignored when the active harness is not Claude. `codex_args` is ignored when the active harness is not Codex. `opencode_args` is ignored when the active harness is not OpenCode.

## agent_doc_dogfood

`agent_doc_dogfood: true` in document frontmatter opts that session into
actionable Agent Doc terminal-failure notifications. Any command failure that
prevents a successful turn boundary emits an `ACTIONABLE_AGENT_DOC_FIX_PROMPT`
with a stable document + failure-class key; the prompt directs the active agent
to fix the underlying Agent Doc defect while preserving retained-capture
no-resubmit/no-recycle constraints. `false` explicitly disables the mode.
When absent, legacy agent-doc source/task path inference remains for backward
compatibility. `dogfood_mode` is accepted as a read-only alias; serialization
uses `agent_doc_dogfood`.

## Project Config

Location: `.agent-doc/config.toml` (relative to project root).

Fields:
- `tmux_session` — the tmux session name bound to this project.
- `agent_doc_auto_compact = <line-threshold>` — explicit opt-in for automatic compaction/reload policies. Session-accretion warnings, repeated no-op closeouts, and Claude skill auto-update must not compact by default when this setting is absent.
- `agent_doc_queue_context_reset = <bool>` — explicit opt-in for accretion-driven fresh-context handoff in direct `run` and Codex continuation diagnostics (`#nm1x-no-preempt-clear`, `#nm1x-codex-clear-parity`). Default off. When enabled and a reset reason is active, direct `run` may start the next backend call from a fresh session and Codex Stop-hook continuation records the suppressed background-clear decision, but the supervisor idle-queue watch must not inject `/clear` for ordinary queue heads. Explicit operator clears and explicit queued slash commands remain the only clear sources. A document-frontmatter `agent_doc_queue_context_reset: true` takes precedence over this project setting.
- `agent_doc_bug_target_document = "<relative-or-absolute .md>"` — optional project-default session document for `#agent-doc-bug` and dogfooding route-failure backlog capture. When absent or empty, bug backlog items stay in the current document. Explicit prompt text such as "Add to the backlog of tasks/bugs.md" takes precedence over this default.
- `agent_doc_clear_threshold = <0..100>` — context-usage percentage at or above which an opted-in direct `run` starts fresh context and Codex Stop-hook continuation records a suppressed background-clear decision before the next dispatch (`#clear-opt-in-threshold`). Resolution: per-document frontmatter `agent_doc_clear_threshold`, then this project setting, then the built-in default of `50`; values are clamped to `0..=100`. The binary owns the threshold value so every harness/editor shares it and surfaces it on the preflight `session_accretion.clear_threshold` field (`session_accretion::clear_threshold_for_doc`). Supported live transcript sources include Claude `usage` JSONL and Codex `token_count` JSONL; missing or unsupported transcript data fails safe (`pct=none clear=false`). This is the configurable companion to `agent_doc_queue_context_reset` (the on/off opt-in).
- `agent_doc_supervisor_auto_recycle = <bool>` — opt-out knob for proactive route-owned-supervisor polling that `execve` hot-reloads onto a freshly-installed binary at an idle / inter-queue-item boundary (`#ctlrecycle` R3 / `#suprecyclequeue` / `#supselfheal`), preserving the live agent child + pane (blue/green, zero-gap). Resolution: the env var `AGENT_DOC_SUPERVISOR_AUTO_RECYCLE` (truthy `1`/`true`/`yes`/`on` force-enables, falsey `0`/`false`/`no`/`off` force-disables), then a per-document frontmatter `agent_doc_supervisor_auto_recycle`, then this project setting, then the built-in default of **ON** (`#supselfheal`). With the default on, a stale supervisor self-retires at the next turn/queue-item boundary — the hands-off self-heal for the freshly-`cargo install`ed-but-still-running-the-old-binary case that otherwise re-files File Cache Conflict / IPC-drift dialogs forever (`#fcc0`/`#ipcdrift`). The recycle is gated to a turn boundary (`prompt_visible && !turn_active`) so a live turn is never dropped, debounced at idle so a momentary lull never thrashes the child, and disabled for the process lifetime after a failed `execve` so a hopeless reload never re-spams. Set the knob falsey to disable proactive polling only: once preflight, generation, streaming, finalize/write, commit, compact, session-check, or closeout proof actually establishes that the supervisor is stale, that integrity condition writes an idempotent CRDT-checkpointed recycle request regardless of this preference and the controller performs it at the next safe boundary. Resolved by `resolve_supervisor_auto_recycle` / `supervisor_auto_recycle_enabled`.
- `agent_doc_agent_change_restart = <bool>` — opt-out knob for frontmatter `agent:` changes to restart the supervised harness at a quiet boundary (`#agentreloadrestart`). Resolution: env var `AGENT_DOC_AGENT_CHANGE_RESTART`, then per-document frontmatter, then this project setting, then the built-in default of **ON**. With the default on, a live supervisor re-resolves the current frontmatter at dispatch-ready prompt boundaries, logs `harness_change_detected` / `agent_restart_boundary_gate`, and performs a fresh-harness restart rather than letting route cold-replace a healthy live actor.
- `skill_runbook_mirrors = ["<project-relative-dir>", ...]` — project-owned directories that also carry copies of the bundled agent-doc runbooks (`#skillinstallstalemirror`). A typical case is a superproject `runbooks/` that the project's own `CLAUDE.md` links to directly. `skill install` refreshes every bundled runbook into each listed directory and `audit-docs` blocks when one drifts from the running binary. Neither ever creates the directory (an absent path is skipped) and neither ever reaps a file agent-doc does not bundle, because these directories also hold project-owned runbooks. Default empty, so an unrelated `runbooks/commit.md` is never rewritten by a project that did not opt in. The four managed harness directories (`.claude/skills/agent-doc/runbooks/`, `.opencode/…`, `.codex/skills/agent-doc/runbooks/`, `.cursor/rules/runbooks/`) are always managed and always exclusive — unbundled Markdown there is reaped on install and blocks on audit. `.codex/runbooks/` is a **retired** location: `skill install` removes the bundled files it left behind and the directory once empty, keeping anything agent-doc never bundled.
- `ssh.profiles.<name>.targets = ["alias-or-host", ...]` — named SSH target groups for ops docs.
- `ssh.docs."<relative/path.md>"` — per-document SSH defaults for known ops docs. Each entry may set `profile = "<name>"`, direct `targets = [...]`, or both. If an entry exists but resolves no targets, preflight/startup must fail closed.
- `documents.include = ["tasks/**/*.md", "plan.md", ...]` — project-relative globs whose matching `.md` files are treated as agent-doc session documents (opt-in gate). Globs support `*` (within a path segment), `?` (one char), and `**` (spans `/`, including zero segments).
- `documents.auto_session_for_all_md = <bool>` — legacy escape hatch restoring the old "every `.md` is a session" behavior. Default `false`.

**Auto-sync:** When the configured `tmux_session` is dead (session no longer exists), the route path falls back to `current_tmux_session()` and auto-updates `config.toml` with the new session name. This prevents stale config after session destruction.

## Terminal host policy

Terminal presentation is configured by an optional `[terminal]` table in global
or project config:

- `host = "auto" | "ide" | "external" | "none"` selects the presentation host.
- `command` is an external-terminal template with `{tmux_command}` substitution.
- `auto_start_tmux` defaults to `true`; `false` refuses to create a missing
  session but still permits attachment to an existing one.
- `attach_command` is the IDE attach template and supports `{session}` and
  `{tmux_command}` substitution.

Document frontmatter `terminal_host` overrides the project host, which overrides
the global host. Project values for the other fields override global values.
Session naming is deliberately absent from `[terminal]`: the sole project
binding remains top-level `tmux_session`, managed by `agent-doc session set`.

The pure resolver consumes config plus typed environment observations. An
already-attached live session resolves to no presentation action. Otherwise,
`auto` selects an available IDE before an external terminal. Explicitly choosing
an unavailable host fails closed; in particular, a headless Coder backend cannot
silently satisfy `external` by choosing an IDE or attempting a local display.

## Opt-in Document Gate

A plain `.md` is **not** auto-converted into an agent-doc session. `route`, `run`, and `start` fail closed before injecting `agent_doc_session:` frontmatter unless the document opts in. The pure predicate `agent_doc_frontmatter::project_config::is_agent_doc_document(rel_path, content, config)` (FFI: `agent_doc_is_session_document(path)`) returns true when ANY of:

1. `documents.auto_session_for_all_md = true` (escape hatch).
2. Frontmatter carries any agent-doc-managed field — `Frontmatter::has_agent_doc_marker()` (e.g. `agent_doc_session`/`session`, `agent_doc_format`, `agent_doc_write`, `agent_doc_mode`, `agent_doc_stream`, `agent`, `resume`, model overrides, `*_args`, `branch`, `queue_active`, `prompt_presets`, `agent_doc_pipeline`). Existing sessions stay sessions.
3. The project-relative path matches a `documents.include` glob.

When none hold, the gate returns an error naming the opt-in paths (`agent-doc init <file>`, an `agent_doc_format:` field, a `[documents] include` glob, or the escape hatch) and **does not mutate the file**. A malformed frontmatter block bypasses the gate so its own contextual YAML parse error surfaces. `agent-doc init <file>` and `agent-doc claim <file>` remain explicit per-file opt-ins that scaffold. Editor plugins should call the FFI gate so **Run Agent Doc** / SubmitAction is hidden for non-opted-in `.md`.

## Socket IPC

Socket-based IPC via PID-scoped Unix domain sockets (`.agent-doc/ipc-<pid>.sock`) is the live editor transport. The editor plugin starts a listener via the `agent_doc_start_ipc_listener()` FFI call on project open. The CLI sender connects to the registered editor endpoint, sends NDJSON messages, and waits for receipts.

**Protocol:** Newline-delimited JSON (NDJSON). Every connection begins with an `ipc_hello` / `ipc_hello_ack` exchange carrying `protocol_version` and the top-level build ID. Both fields must match before an editor intent reaches the plugin callback. A missing or mismatched handshake returns a terminal rejected receipt, so old/new process skew fails before document mutation. `reload_library` is the only pre-handshake control intent: it is non-mutating and permits a newer install to replace a listener too old to negotiate the current protocol.

Post-handshake message types:
- `{"type": "apply_canonical", "file": "...", "patches": [...], ...}` — apply canonical component deltas
- `{"type": "reposition", "file": "..."}` — reposition boundary marker
- `{"type": "refresh_vcs"}` — trigger VCS/VFS refresh

**Detached behavior:** If no editor owns the document, the authority resolver may project to disk. If an editor owns it but its PID-scoped socket is unavailable or incompatible, the write remains retained and fails closed; there is no file-IPC fallback.

## IPC Write Verification

After the IDE plugin consumes an IPC patch file:
1. **File-change check:** If the document file is unchanged on disk, the plugin failed to apply — the CLI fails closed and retains retry state instead of writing the document directly.
2. **Content verification:** If the document changed but none of the patch content appears in the result, the plugin partially failed — the CLI fails closed and retains retry state instead of writing the document directly.
3. **Force-disk cleanup:** When `--force-disk` is set, any pending IPC patch files are deleted before disk write to prevent the plugin from applying stale patches (double-write prevention).
4. **Claimed stale-patch cleanup:** If the CLI rejects an IPC patch because the cycle already committed, it writes `.agent-doc/claimed-patches/<patch_id>`. Plugins must treat that sentinel as a durable per-patch skip signal and delete the stale patch file instead of replaying it into the editor buffer; the sentinel must survive repeated watcher scans so multiple editor consumers cannot race a stale patch into a duplicate external edit.
5. **Append patch idempotence:** Editor-side component patch application must treat append-mode patches as replay-safe. Before appending a patch into `agent:exchange` or another append component, the native/shared patcher and editor fallbacks compare the normalized existing component body against the normalized patch body, ignoring transient `(HEAD)` response-heading markers and `agent:boundary` comments. If the patch body is already present, the plugin must leave the document unchanged instead of duplicating the exchange. JetBrains File Cache Conflict retries also revalidate against the current disk file and committed `HEAD`; `HEAD` is accepted as proof only when the disk file still matches it, so stale editor buffers are not left missing a committed response.
6. **Snapshot freshness cleanup:** On pending-patch pickup, if `.agent-doc/snapshots/<hash>.md` is newer than the patch file, the patch is stale and must be deleted without apply.
7. **Malformed patchback rejection:** Before capturing or sending a named socket intent, the CLI validates the parsed template patchback shape. If patch/replace markers are present outside code blocks but no closed patch blocks parse, the write fails closed with `template_patchback_malformed_rejected`; no transaction or editor mutation is created.
8. **Forensic visible-write logging:** Socket IPC sends log `ipc_socket_attempt` with patch counts, synthesized IPC patch counts, unmatched lengths, baseline length, normalization target count, and unmatched marker count. Successful lazily visible-write receipts log `ipc_socket_visible_write` with receipt source and pre-write disk hashes/lengths before snapshot persistence.

## Sync Layout Authority

`sync_after_claim()` uses editor-provided `col_args` when available (authoritative layout from the IDE plugin). Only falls back to registry-based file discovery when no `col_args` given. This prevents stale registry entries from creating incorrect multi-pane layouts.
When that helper runs under an injected tmux handle (for example isolated route verification), the follow-up sync must stay on that same tmux server instead of falling back to the process-default server. Otherwise verification can accidentally stash panes in the operator's live `agent-doc` window while trying to reconcile a test-only layout.

## Document State Model

A document has one live text authority and three projections during a write cycle:

| State | Location | Owner | Purpose |
|-------|----------|-------|---------|
| **Lazily current** | in-process CRDT replicas | Lazily + editor replica | Sole live text, including unsaved operator edits and semantic agent cells. |
| **Transaction state** | `.agent-doc/state.db` | Binary state machine | Baselines, captured responses, operation ids, phases, receipts, queue tombstones, and recovery checkpoints. |
| **Disk projection** | document file | Binary/editor save integration | Durable visible projection after `ReplicaVisible`; never an attached-editor merge authority. |
| **Snapshot** | `.agent-doc/snapshots/<hash>.md` | Binary | Cold recovery/audit image emitted after verified convergence. |

There is no live-buffer, baseline, capture, pending, cycle JSON, or CRDT file
sidecar and no compatibility import for one. The structured CRDT recovery
checkpoint is stored in `state.db`; file snapshots cannot override Lazily
current text or the transaction ledger.

**Consistency invariants:**

- Preflight records the exact Lazily generation and semantic baseline in `state.db`.
- Agent writes rebase node-keyed intent onto the newest Lazily generation; an
  operator deletion or edit wins on a same-node conflict.
- The delivery state advances monotonically through `IntentCaptured`,
  `CanonicalApplied`, `ReplicaAccepted`, `ReplicaVisible`, `DiskProjected`, and
  `Committed`. A later phase can be retried idempotently; it cannot send the
  transaction backward or expose a terminal capture as active.
- Disk projection happens only after visible-replica proof. Detached documents
  may project directly through the same authority resolver.
- After commit, Git `HEAD`, the disk projection, and the cold snapshot agree on
  the clean boundary shape. Lazily may additionally retain editor-only display
  annotations.
