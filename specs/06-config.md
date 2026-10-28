> Extracted from SPEC.md — see [index](../SPEC.md)

# Config

Location: `{XDG_CONFIG_HOME}/agent-doc/config.toml` (default `~/.config/agent-doc/config.toml`).

`tmux_bin` is a machine-wide tmux client override. Resolution is project
`tmux_bin`, global `tmux_bin`, then `PATH`; the selected source is exposed by
`agent-doc env --json`. Every tmux-owning runtime path uses this same process-wide
resolution.

Fields: `default_agent`, `agent_args`, `claude_args`, `codex_args`, `opencode_args`, `codex_network_access`, `[queue_icons]`, `[agents.{name}]` with `command`, `args`, `result_path` (reserved), `session_path` (reserved).

## Queue projection prefixes (`[queue_icons]`)

Every visible queue-state and queue-priority prefix is configurable in the global
config. Values are strings, not scalar characters: a prefix may contain several
Unicode code points or an ASCII label such as `"OUT::"`. Values must be non-empty,
single-line, free of surrounding whitespace, and distinct after defaults are
resolved. Invalid palettes fail command startup instead of making queue identity
ambiguous.

```toml
[queue_icons]
pending = "📥"
blocked = "⛔"
claimed = "🟡"
running = "🚧"
reported = "☑️"
lost = "⚠️"
reconciling = "🔄"
unknown = "❔"
skipped = "⏭️"
awaiting_operator = "⏳"
operator_action = "🙋"
prioritized = "📌"
agent_prioritized = "📍"
```

Every field is optional and falls back independently to the shown built-in
default. The configured palette governs both projection and parsing for the
process. Built-in lifecycle spellings remain readable as migration aliases, so
changing a prefix does not change queue work identity or strand an existing
session document. In particular, the former reported/outbox prefixes `📫` and
`📤` remain accepted input aliases for the default `☑️`.

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

## AGENT_DOC_SOCKET_DIR

Optional environment override for editor endpoint sockets. By default the IDE
listener binds `<project_root>/.agent-doc/ipc-<pid>.sock`. Coder workspaces whose
persistent project volume is backed by 9p or another filesystem without
`AF_UNIX` support must set `AGENT_DOC_SOCKET_DIR` to a writable, local,
project-specific runtime directory. Listener startup fails immediately with
this recovery instruction when the selected directory cannot host the socket.

## AGENT_DOC_ROOT_CEILING_DIRECTORIES

Optional environment bound on `.agent-doc` project-root discovery, with
`GIT_CEILING_DIRECTORIES` semantics: a `PATH`-style list of absolute
directories (relative and empty entries are ignored). Discovery examines the
starting directory and then each parent, but never climbs into a ceiling
directory or anything above it. Unset, discovery walks to the filesystem root
as before. The test harness (`make test`/`check`, the nextest
`test-isolation` setup script) sets it to `<TMPDIR>:<workspace>` (appending
this checkout's own entries to an inherited list that lacks them, so a ceiling
exported by another checkout never replaces this one's; `#checktmpdirleak`) and refuses a
`TMPDIR` with an `.agent-doc` ancestor (`#testisolationtests`); a cargo test
binary without it bounds in-process discovery at its temp root.

## AGENT_DOC_LOG

Optional `tracing` filter (for example `debug` or `agent_doc::preflight=debug`).
Unset, nothing is logged to a file. Set, the log rolls daily as
`debug.log.<date>` in `<root>/.agent-doc/logs/`. `<root>` is the nearest
project root that ceiling-aware discovery resolves from the working directory,
so `AGENT_DOC_ROOT_CEILING_DIRECTORIES` bounds it. `logs/` is created when it
is missing. With no project root inside the ceiling, file logging is disabled
with a stderr note. It never falls back to a user-level directory or to an
ancestor project above the ceiling (`#tracingrootceiling`).

## Project prompt presets

`.agent-doc/project.md` is the single project-wide source of inherited prompt
presets. Its YAML frontmatter uses the same `presets:` / `prompt_presets:` shape
as a session document. Presets are resolved dynamically on every preset-aware
operation; they are never copied into existing documents and do not require a
template-sync command.

Resolution starts at the nearest project root selected by the existing
`.agent-doc` project-root discovery, then walks through its ancestors within the
configured root ceiling. The effective map starts with the farthest ancestor's
`.agent-doc/project.md`, overlays each nearer project's declarations by name,
then overlays the target document. A nearer project or document declaration
wins, including an unset YAML entry (`'#name':`) that is still being typed; such
an entry masks the inherited value. Structured preset bodies and their `runbook`
metadata inherit together.

A missing `project.md` means there are no project defaults. A read failure or
malformed project frontmatter is an actionable preset-resolution error; a
preset-aware admission must not silently continue with a stale or partial map.
`agent:rules` is not part of that frontmatter overlay. Rules use the same
ancestor walk: farthest project file first, then each nearer project file,
then the session. A nested repository's `.git` does not hide an ancestor
project file. A missing project file is skipped. A rules body that does not
parse contributes nothing instead of failing the preset map.

Nested repositories therefore inherit a workspace parent's presets and rules
while their own `project.md` remains the nearer overlay. The similarly named
`.agent-doc/config.toml` remains runtime configuration, not a prompt
preset source, and `.agent-doc/project.yaml` is not an alternative spelling.

New-document templates may reference or demonstrate these presets, but must not
materialize inherited preset values into generated documents. A local preset is
written only when the new document intentionally overrides the project default.

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

## dogfood / agent_doc_dogfood

Dogfood routing is document-local and off by default. The session value wins;
when absent, the nearest explicit value in `.agent-doc/project.md` is inherited.
Absence at both layers is disabled. Accepted forms are:

```yaml
dogfood: false                         # disabled
dogfood: true                          # current document; manual queueing
dogfood: tasks/agent-doc/bugs.md       # project-relative target; manual queueing
dogfood:
  path: tasks/agent-doc/bugs.md        # optional; absent = current document
  auto: true                           # optional; default false
  prompt: >-                           # optional; default action is diagnose + fix
    File a GitHub issue with the diagnostic instead of changing code.
```

`auto: true` applies only to generated, stable-keyed dogfood findings; ordinary
backlog work is never activated. A self-target is legal because stable keys
deduplicate it and routing never invokes agent-doc recursively. The canonical
serialized key is the public `dogfood`; `agent_doc_dogfood` and `dogfood_mode`
remain read aliases.

`prompt` replaces only the action sentence in the terminal-failure notification.
The binary-owned `ACTIONABLE_AGENT_DOC_FIX_PROMPT` marker, stable issue class/key,
failure context, and retained-capture/no-resubmit/no-recycle safety guidance are
always preserved. This allows a remote or review-only dogfood runner to file a
GitHub issue instead of modifying code directly. Leading and trailing whitespace
is trimmed; an absent, empty, or whitespace-only value uses the default
“diagnose and fix” action. Like `path` and `auto`, `prompt` belongs to the selected
session-or-project dogfood value; map fields do not merge across precedence layers.

Integrations query the binary-owned resolver with
`agent-doc dogfood policy <FILE> --json`. Its typed report distinguishes a
disabled policy from an enabled destination and includes `auto` and
`self_target`; watchers must not reimplement frontmatter parsing, inheritance,
or project-relative path resolution.

`agent-doc dogfood set <FILE> [--path <PATH>] [--auto <BOOL>] [--prompt <TEXT>]` is the
binary-owned mutation boundary. It reads and writes through live document
authority and changes only the local `dogfood:` field while preserving
unrelated frontmatter and body bytes. An omitted path means the current
document; omitted `auto` means false; omitted or blank `prompt` selects the
default action. Explicit paths must be project-relative
Markdown paths without parent traversal, and the resolved policy is validated
before the write crosses the authority boundary. The mutation remains visible
to the ordinary session closeout rather than advancing its baseline out of band.

This switch gates feedback routing, not diagnosis. Queue lifecycle invariants
are checked for every session. Cosmetic projection drift has the single safe
repair of re-projecting the durable claim state; an unconfirmed worker remains
typed `Unknown` and produces a diagnostic instead of an invented `🚧` state.

## agent_doc_scratch

`agent_doc_scratch: true` in document frontmatter declares a deliberate
scratch/proof document that lives under a git-ignored path, such as the live
captured-splice proof scratch in `tmp/captured-splice-live/`
(`#splicectlcommit`). It changes only how the commit's ignored-path refusal is
reported: an untracked path `.gitignore` matches is still never staged and its
cycle still closes as `abandoned` / `commit_refused_ignored_path`, but the commit
logs `commit_skipped_ignored_scratch outcome=clean_skip` instead of
`commit_skipped_ignored_path` + `commit_error`, so proof tooling and the dogfood
watch do not read the expected skip as a failure. It has no effect on a tracked
or non-ignored path (those commit normally), never redirects the commit to
another repository, and is read from the document itself, so every committing
process (CLI, recovery, or the project controller under a live editor) agrees.
`false`, absent, or unparsable frontmatter keep the strict refusal receipts.

## Project Config

Location: `.agent-doc/config.toml` (relative to project root).

A document's project config is its **store's** config, never the process cwd's
project (`#loadprojectfordoccwd`). `load_project_for_doc` resolves the store with
`project_store_for_doc`, which is `agent_doc_project_root_io::project_root_or_file_parent`:
the nearest `.agent-doc` project of the canonical document, or for a document with
no `.agent-doc` ancestor its canonical parent directory, the store start
(`#startcwdfallback`), claim (`#claimwrongstore`), the route and sync's
recovered-owner bind (`#syncnorootfallback`) bind it in. A path that cannot be
canonicalized yet (a document not created) keeps the lexical project walk. A
rootless document's store has no config until it is created, so the document
reads defaults even when run from inside another configured project, and the
read creates nothing. Only process-level reads with no document (`load_project`,
`project_tmux_session`, `tmux_bin_resolution`, `agent-doc session`) and
`describe-image` of an image outside any project (an image is not a session
document and has no store) use the cwd's project.

The same store answers every other document-scoped read (`#cwdfallbackrest`):
template patching's `[components]` modes and `max_lines` (`agent-doc-template-io`
calls `load_project_for_doc`; it no longer walks up for `.agent-doc/config.toml`
on its own, which ignored the root ceiling `AGENT_DOC_ROOT_CEILING_DIRECTORIES`
and fell back to the cwd's project), `terminal`'s session policy, registry scan
and `tmux_session` pin, scoped `resync`/`fix`, and claim's `--window` fallback
([Session and tmux commands](07-session-tmux-commands.md)). Discovery is the
ceiling-aware walk (`agent_doc_fs::find_project_root`), so a project above a
ceiling never owns a document below it. A registry read of a store that does not
exist yet (`agent_doc_session_registry_io::load_existing_in`) answers empty and
creates nothing.

Fields:
- `default_agent` — the default agent backend for documents in this project. It
  overrides the global `default_agent`; document frontmatter and an explicit
  CLI `--agent` take precedence over both.
- `tmux_session` — the tmux session name bound to this project.
- `tmux_sessions = ["<name>", ...]` — allowed tmux sessions for a multi-session project (GH #17, `ghmultitmux`). Empty or absent (the default) keeps the single-session model unchanged. When non-empty, one editor manages panes across every listed session: route, start, claim, sync, layout, resync and `terminal` resolve each document to an allowed session and fail closed on any target outside the list. `tmux_session` then acts as the default pin for unbound documents and must itself be listed. Per-document topic binding uses `tmux_session:` frontmatter, which is honored only in multi-session projects. See [Session Routing](08-session-routing.md#multi-session-projects-ghmultitmux) and `runbooks/multi-tmux-sessions.md`.
- `tmux_bin` — project override for the tmux client; takes precedence over the global machine value.
- `agent_doc_auto_compact = <line-threshold>` — explicit opt-in for automatic compaction/reload policies. Session-accretion warnings, repeated no-op closeouts, and Claude skill auto-update must not compact by default when this setting is absent.
- `agent_doc_queue_context_reset = <bool>` — explicit opt-in for accretion-driven fresh-context handoff in direct `run` and Codex continuation diagnostics (`#nm1x-no-preempt-clear`, `#nm1x-codex-clear-parity`). Default off. When enabled and a reset reason is active, direct `run` may start the next backend call from a fresh session and Codex Stop-hook continuation records the suppressed background-clear decision, but the reset reason alone never makes the supervisor idle-queue watch inject `/clear`. Between-item resets are governed by queue auto-clear instead (`#bgclearpolicy`): on by default for every active `agent:run`, explicitly declared by bare `clear` in generated templates, and opted out by the legacy `disable-auto-clear` attribute (there is no config key), see [Supervisor](supervisor.md). Explicit operator clears, explicit queued slash commands, and queue auto-clear are the only clear sources. A document-frontmatter `agent_doc_queue_context_reset: true` takes precedence over this project setting.
- `agent_doc_bug_target_document = "<relative-or-absolute .md>"` — optional project-default session document for `#agent-doc-bug` and dogfooding route-failure backlog capture. When absent or empty, bug backlog items stay in the current document. Explicit prompt text such as "Add to the backlog of tasks/bugs.md" takes precedence over this default.
- `agent_doc_clear_threshold = <0..100>` — context-usage percentage at or above which an opted-in direct `run` starts fresh context and Codex Stop-hook continuation records a suppressed background-clear decision before the next dispatch (`#clear-opt-in-threshold`). Resolution: per-document frontmatter `agent_doc_clear_threshold`, then this project setting, then the built-in default of `50`; values are clamped to `0..=100`. The binary owns the threshold value so every harness/editor shares it and surfaces it on the preflight `session_accretion.clear_threshold` field (`session_accretion::clear_threshold_for_doc`). Supported live transcript sources include Claude `usage` JSONL and Codex `token_count` JSONL; missing or unsupported transcript data fails safe (`pct=none clear=false`). This is the configurable companion to `agent_doc_queue_context_reset` (the on/off opt-in).
- `agent_doc_steering_debounce_ms = <ms>` — quiet period before a mid-turn operator edit counts as settled steering (`#midturn-steering`). The PostToolUse steering hook and `agent-doc steering` hold an edited queue line or exchange prompt until the document has been quiet for this long (or the item itself has been observed unchanged for this long), and always hold plainly incomplete shapes (unbalanced code fence, trailing `#`, unclosed `[#…`, empty bullet). Resolution: per-document frontmatter `agent_doc_steering_debounce_ms`, then this project setting, then the built-in default of `2500`.
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
2. Frontmatter carries any agent-doc-managed field — `Frontmatter::has_agent_doc_marker()` (e.g. `agent_doc_session`/`session`, `agent_doc_format`, `agent_doc_write`, `agent_doc_mode`, `agent_doc_stream`, `agent`, `resume`, model overrides, `*_args`, `branch`, `queue_active`, `prompt_presets` (or its `presets` alias), `agent_doc_pipeline`). Existing sessions stay sessions.
3. The project-relative path matches a `documents.include` glob.

When none hold, the gate returns an error naming the opt-in paths (`agent-doc init <file>`, an `agent_doc_format:` field, a `[documents] include` glob, or the escape hatch) and **does not mutate the file**. A malformed frontmatter block bypasses the gate so its own contextual YAML parse error surfaces. `agent-doc init <file>` and `agent-doc claim <file>` remain explicit per-file opt-ins that scaffold. Editor plugins should call the FFI gate so **Run Agent Doc** / SubmitAction is hidden for non-opted-in `.md`.

## Socket IPC

Socket-based IPC via PID-scoped Unix domain sockets (`.agent-doc/ipc-<pid>.sock`) is the live editor transport. The editor plugin starts a listener via the `agent_doc_start_ipc_listener()` FFI call on project open. The CLI sender connects to the registered editor endpoint, sends NDJSON messages, and waits for receipts.

**Protocol:** Newline-delimited JSON (NDJSON). Every connection begins with an `ipc_hello` / `ipc_hello_ack` exchange carrying `protocol_version` and the top-level build ID. Both fields must match before an editor intent reaches the plugin callback. A missing or mismatched handshake returns a terminal rejected receipt, so old/new process skew fails before document mutation. `reload_library` is the only pre-handshake control intent: it is non-mutating and permits a newer install to replace a listener too old to negotiate the current protocol.

**Build ID (`#ipcverhandshake`):** `<version>+<16 hex>`, where the suffix is a SHA-256 digest of the workspace's build-relevant sources (`agent-doc-hash`'s `source_digest`; `build.rs` emits it as `AGENT_DOC_BUILD_ID` and declares one `rerun-if-changed` per hashed input, plus each directory holding one so a newly created source re-runs it). Cargo scans a declared directory recursively, so the build script never declares the workspace root or any directory whose subtree holds build or runtime state (`target/`, `node_modules/`, `.agent-doc/`, ...): declaring the root watched `target/` itself, left the root package stale on every cargo invocation, and let a second cargo process on the same checkout relink the test binaries and `target/debug/agent-doc` under a running nextest (`[double-spawn] failed to exec`, `stale_binary_client` controller recycles; `#flakyintegrate0494`). It is compared for equality only — never ordered. A rejection is consumed throughout the system as proof that two processes run different code, so the identity must be a function of the code and nothing else. It previously was the wall-clock second at which the build script ran, which proved neither direction: identical sources built into two target directories (`make check` uses `target/`, `make install` uses `target/local-install`) were rejected as skewed, while an edit to any member package was admitted as identical, because a cargo build script is never re-run for a change in a different package. Both directions are modelled in `formal/tla/IpcBuildIdentity.tla`, each with its own must-violate config; the cost of a false rejection to an attached document is `formal/tla/EditorReplicaStrand.tla`.

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
6. **Ledger freshness cleanup:** On pending-patch pickup, the controller's typed operation receipt and document generation decide whether the patch is stale; no filesystem snapshot timestamp participates.
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

There is no live-buffer, baseline, capture, pending, cycle JSON, markdown
snapshot, or CRDT file sidecar and no compatibility import for one. Baselines
and structured CRDT recovery checkpoints are stored in `state.db`; no copied
document path exists that could override Lazily current text or the transaction
ledger.

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
- After commit, Git `HEAD`, the disk projection, and the typed ledger baseline
  agree on the clean boundary shape. Lazily may additionally retain editor-only
  display annotations.
