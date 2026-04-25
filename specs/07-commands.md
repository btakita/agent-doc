> Extracted from SPEC.md — see [index](../SPEC.md)

# Commands

## run

`agent-doc [run] <FILE> [-b] [--agent NAME] [--model MODEL] [--dry-run] [--no-git]`

1. Compute diff → 2. Resolve document mode from frontmatter (`resolve_mode()`, default template) → 3. Build the matching append/template prompt (diff + full doc) → 4. Branch if `-b` → 5. Send to agent → 6. Durably capture the final parsed response in `.agent-doc/captures/<doc-hash>/<cycle-id>.json` → 7. Apply the response through the matching append/template write path → 8. Update session resume ID → 9. Persist the final post-write cycle state as `write_applied` → 10. Save snapshot → 11. Commit the response write and require the cycle to close in `committed` for git-backed runs

First run prompt wraps full doc in `<document>` tags. Subsequent wraps diff in `<diff>` tags + full doc in `<document>`.

`agent-doc <FILE>` and `agent-doc run <FILE>` are equivalent. Both dispatch by document mode from frontmatter, with template as the default when no explicit format is present.

**Imperative-directive guard:** if the pending user diff contains executable directives like `do #id`, `run tests`, `build + install`, `commit + push`, a one-word approval such as `go`, or natural-language pending-item task text that begins with an imperative verb (for example `[#n8q4] Fix the cross-repo ...`), `run` rejects status-only/meta-only agent replies. The response must include either concrete execution evidence (for example commands, verification/commit sections, or file-path evidence) or a concrete blocker.

**Interrupted-run contract:** if `run` writes the final response to disk but stops before the post-write commit finishes, the recorded cycle state must already be `write_applied` with the final file/snapshot hashes. That lets `agent-doc preflight` or `repair` finish the pending commit deterministically instead of misclassifying the cycle as stale `response_captured` drift.

## init

Two modes:

**No-arg (project init):** `agent-doc init` — checks prerequisites, creates `.agent-doc/snapshots/` and `.agent-doc/patches/` directories, and installs `.claude/skills/agent-doc/SKILL.md`. Idempotent. Run once per project before creating session documents.

**With file (document scaffold):** `agent-doc init <FILE> [TITLE] [--agent NAME]` — scaffolds frontmatter + `## User` block. Fails if file already exists. Lazily runs project init first if `.agent-doc/` does not exist.

## install

`agent-doc install [--editor jetbrains|vscode] [--skip-prereqs] [--skip-plugins]` — system-level setup.

1. **Prerequisite check** (unless `--skip-prereqs`): verifies `tmux` and `claude` are on `PATH`; prints ok or MISSING with install hint for each. Does not fail — only warns.
2. **Editor plugin install** (unless `--skip-plugins`):
   - If `--editor` is given, installs only that editor's plugin.
   - Otherwise, auto-detects installed editors: JetBrains (checks `~/.local/share/JetBrains/` on Linux, `/Applications/IntelliJ*` on macOS) and VS Code family (`cursor`, `codium`, `code`).
   - If no editors detected, prints a hint to use `--editor` and exits without error.
   - Calls `crate::plugin::install(editor)` for each detected editor.
   - Prints a summary of installed and failed editors.

## diff

`agent-doc diff <FILE>` — prints unified diff to stdout.

## reset

`agent-doc reset <FILE>` — clears session ID, deletes snapshot.

## clean

`agent-doc clean <FILE>` — squashes all `agent-doc:` commits for file into one via `git reset --soft`.

## audit-docs

`agent-doc audit-docs [--root DIR]` — checks CLAUDE.md/AGENTS.md/README.md/SKILL.md for tree path accuracy, line budget (1000), staleness, and actionable content. Exit 1 on issues.

`--root DIR` overrides auto-detection of the project root directory. Without it, the root is resolved via project markers (Cargo.toml, package.json, etc.), then `.git`, then CWD fallback.

When `agent-doc audit-docs` is launched from an outer repo via a nested crate checkout (for example `cargo run --manifest-path src/agent-doc/Cargo.toml -- audit-docs` from a monorepo root), the default scope prefers the running `src/agent-doc` crate root over the outer repo root. That keeps discovery aligned with the crate being audited instead of the caller's larger checkout.

## start

`agent-doc start <FILE>` — start the configured harness in a new tmux pane and register the session.

1. Ensure session UUID in frontmatter (generate if missing)
2. Read `$TMUX_PANE` (must be inside tmux)
3. Register session → pane in `sessions.json`
4. Resolve harness args from `agent_args` / harness-specific aliases, then auto-append harness-native `--add-dir` entries for any external git metadata directories needed by submodule documents (`.git/modules/...` and the superproject `.git` when applicable)
5. Exec the configured harness (replaces process)

## route

`agent-doc route <FILE> [--pane P]` — route a `/agent-doc` command to the correct tmux pane.

1. Prune stale entries from `sessions.json`
2. Ensure session UUID in frontmatter (generate if missing)
3. Look up pane in `sessions.json`
4. If pane alive → send `/agent-doc <FILE>` via `send_keys`, then Enter verification loop (polls for command text disappearance every 300ms, retries Enter on each poll, up to 5s timeout), focus pane
5. If pane dead (previously registered) → lazy-claim to active pane in `claude` tmux session (or `--pane P`) **only if the candidate pane is running an agent process** (`agent-doc`, `claude`, `node`). Non-agent panes (corky, shells, etc.) are skipped — falls through to auto-start. Unregistered files skip lazy-claim entirely.
6. If no active pane available → auto-start cascade (see below), register, wait up to 30s for Claude `❯` prompt via `pane_has_prompt()` with ANSI stripping, then send command

**Session validation:** If `tmux_session` references a non-existent tmux session, route logs a warning and falls back to the default session. It does NOT create new tmux sessions. The fallback order is: current tmux session (if running inside tmux) → default `claude` session.

> **Deprecation note:** `tmux_session` in frontmatter is deprecated. The tmux session is now determined at runtime: `--window` argument (sync), `current_tmux_session()` (route/start), or future `.agent-doc/config.toml` settings. The field is still read for backward compatibility and auto-repaired by sync. It will be removed in a future version.

**Auto-start algorithm (`auto_start_in_session`):**
1. **Startup lock:** Check `.agent-doc/starting/<hash>.lock`. If exists and age < 5s → skip (prevents double-spawn when sync fires twice rapidly). Create lock file before proceeding. Best-effort: skipped if file doesn't exist or hash fails.
2. Read `tmux_session` from the document's frontmatter (fall back to default `claude` session name)
3. Find a split target pane:
   - **Sync path** (`skip_wait=true`): pick the split target by column position — first pane in the agent-doc window for left-column files, last pane for right-column files. This places the new pane adjacent to its column neighbors.
   - **Route path** (`skip_wait=false`): search `sessions.json` for any registered pane alive in the target session.
4. If found → `tmux split-window` alongside that pane (`-dbh` for left-column, `-dh` for right-column)
5. If split-window fails → fall back to creating a new window
6. If no split target found → create a new window via `tmux new-window` (the session may not exist yet, in which case a new session is created)

## claim

`agent-doc claim <FILE> [--position left|right|top|bottom] [--window W] [--pane P]` — claim a document for a tmux pane.

1. Ensure session UUID in frontmatter (generate if missing)
2. **Resolve effective window** (see Window Resolution below)
3. Determine pane: `--pane P` overrides, else `--position` resolves via tmux pane geometry, else `$TMUX_PANE`
4. Register session → pane in `sessions.json`, including window ID

Unlike `start`, does not launch Claude — the caller is already inside a Claude session. `--position` is used by the JetBrains plugin to map editor split positions to tmux panes.

**Binding invariant enforcement:** If the target pane is already claimed by a different session (and the pane is alive), `claim` provisions a new pane for this document instead of erroring. This enforces the Binding invariant (§8.5): "never commandeer another document's pane." Use `--force` to explicitly overwrite the existing claim (discouraged — breaks the Binding invariant unless the old document is abandoned).

**Default components on claim:** For new template documents, `agent-doc claim` scaffolds `<!-- agent:status patch=replace -->` and `<!-- agent:exchange patch=append -->` components by default.

**Window Resolution:**

When `--window W` is provided:

1. Check if window `W` is alive (`tmux list-panes -t W`)
2. If alive → use `W` (no change)
3. If dead → scan `sessions.json` for entries with matching project `cwd` and non-empty `window` field. For each, check liveness. Use first alive match.
4. If no alive windows found → fall through to no-window behavior (position detection without window scoping)

This prevents the JetBrains plugin from hitting persistent error balloons when a tmux window dies. The same fallback pattern is used in `sync.rs` for dead `--window` handling.

**Snapshot initialization:** After registration, saves a snapshot with empty exchange content (via `strip_exchange_content`). This ensures existing user text in the exchange becomes a diff on the next run, rather than being absorbed into the baseline.

**Notifications:**
- `tmux display-message` — 3-second overlay on the target pane showing "Claimed {file} (pane {id})"
- `.agent-doc/claims.log` — appends `Claimed {file} for pane {id}` for deferred display by the SKILL.md workflow on next invocation

## focus

`agent-doc focus <FILE> [--pane P]` — focus the tmux pane for a session document.

1. Read session UUID from file's YAML frontmatter (or use `--pane` override)
2. Look up pane ID in `sessions.json`
3. Run `tmux select-window -t <pane-id>` then `tmux select-pane -t <pane-id>`

Exits with error if the pane is dead or no session is registered.

## layout

`agent-doc layout <FILE>... [--split h|v] [--window W]` — arrange tmux panes to mirror editor split layout.

1. Resolve each file to its session pane via frontmatter → `sessions.json`
2. If `--window` given, filter to panes registered for that window only
3. Pick the target window (the one containing the most wanted panes; tiebreak: most total panes)
4. Break out only registered session panes that aren't wanted (shells and tool panes are left untouched)
5. Join remaining wanted panes into the target window (`tmux join-pane`)
6. Focus the first file's pane (the most recently selected file)

`--split h` (default): horizontal/side-by-side. `--split v`: vertical/stacked. Single file falls back to `focus`. Dead panes and files without sessions are skipped with warnings.

## resync

`agent-doc resync [--fix]` — validate sessions.json against live tmux panes.

**Always (dry-run and --fix):**
1. Load `sessions.json`, prune entries with dead panes (delegates to `tmux_router::prune()`)
2. Purge idle stash windows: kill `stash`-named windows where all panes run idle shells (`zsh`, `bash`, `sh`, `fish`) and last activity was >30s ago
3. Log orphaned `claude`/`stash` windows (all panes unregistered) for diagnostics

**Issue detection (alive panes only):**
4. **Wrong-process:** Pane is running a process not in the allowlist (`agent-doc`, `claude`, `node`) and not an idle shell (`zsh`, `bash`, `sh`, `fish`)
5. **Wrong-session:** Pane is in a different tmux session than the document's `tmux_session` frontmatter field. Skipped if no `file` path or no `tmux_session` in frontmatter. Wrong-process panes are not also checked for wrong-session.
6. **Wrong-window:** Pane is in a different non-stash window from the majority of panes sharing the same tmux session. Majority-window is computed by count; ties broken arbitrarily. Panes already in a stash window (`stash`, `stash-2`, etc.) are excluded from this check.

**Without `--fix`:** Reports issues to stderr with "run with --fix to resolve".

**With `--fix`:**
- Wrong-session panes: kills the pane via `tmux kill-pane`, removes registry entry. Next `route` auto-starts in the correct session. With `--session <name>`: uses `join-pane` to relocate the pane to the named session instead of killing it; registry entry is preserved (pane ID is stable). Falls back to deregister if no active pane found in target session.
- Wrong-process panes: removes registry entry only (does not kill the foreign process). Next `route` auto-starts a new pane.
- Wrong-window panes: moves the pane into the stash window via `stash_pane` (does not deregister). The pane stays alive; the next `sync` or `layout` rejoins it into the correct window.

**Non-agent process guard (route):** `is_agent_process()` gates both the wrong-session recovery path and the lazy-claim path (Strategy 2). A pane running corky, a shell, or any non-agent process is never stashed, rescued, or claimed — agent-doc provisions a fresh pane instead. Prevents foreign processes from being dragged across tmux sessions during route/sync.

**Stash window naming:** Stash windows are named `stash`. When tmux auto-deduplicates a name collision the window becomes `stash-2`, `stash-3`, etc. All names matching `stash` or `stash-*` are treated as stash windows (checked by `is_stash_window_name`). `resync` purges stash windows where all panes are idle shells and last activity was >30s ago.

**Auto-start stash overflow (route):** When `auto_start_in_session` tries `split-window` alongside a registered pane and the split fails (e.g. minimum pane size constraint), it falls back to `tmux new-window` then immediately calls `stash_pane` to move the new pane into the stash window — avoiding a visible throwaway window in the session.

**Automatic pruning:** `resync::prune()` (step 1 only — no issue detection or fixing) runs automatically before `route`, `sync`, and `claim` operations. Uses bulk metadata fetching (2 subprocess calls: `list-windows -a` + `list-panes -a`) instead of per-pane queries. Stranded panes (no valid return target) are deregistered on first failure to prevent repeated expensive lookups. **Stash pane safety:** unregistered agent processes (`agent-doc`, `claude`, `node`) in stash windows are never auto-killed — only idle shells are purged. This prevents loss of active Claude sessions when the registry goes stale.

## prompt

`agent-doc prompt <FILE>` — detect permission prompts from a Claude Code session.

- Captures tmux pane content, strips ANSI, searches bottom-up for footer containing `"to cancel"`
- Supports two option formats: bracket `[N] label` (legacy) and numbered list `N. label` (Claude Code v2.1+)
- Returns JSON: `{ "active": bool, "question": str, "options": [...], "selected": int }`
- `--answer N` navigates to option N via arrow keys and confirms with Enter
- `--all` polls all live sessions, returns JSON array of `PromptAllEntry` objects
- Debug: `AGENT_DOC_PROMPT_DEBUG=1` logs last 5 non-empty lines of each captured pane to stderr

## commit

`agent-doc commit <FILE>` — selective commit with auto-generated timestamp.

1. Load the snapshot for the file (the document state after the last `agent-doc write`)
2. If snapshot exists:
   a. Strip any transient `(HEAD)` suffixes from the snapshot copy used for staging
   b. Write the clean snapshot to git's object database via `git hash-object -w --stdin`
   c. Stage via `git update-index --add --cacheinfo 100644,<hash>,<file>`
   d. Result: snapshot content (agent response) is committed; plain user edits in the working tree stay uncommitted
   e. Narrow repair path: if the live document is ahead of the snapshot due to a missed agent-doc mutation, `commit` first refreshes the snapshot from the live file, then stages it. The repair only triggers when the redacted component structure is unchanged and the drift looks like an agent-owned `status` change and/or an appended `### Re:` block and/or a `pending` stable-ID superset. Plain user-prompt drift is not absorbed.
   f. Historical lower bound: if `HEAD` already contains a safe historical agent-owned response growth beyond the stale snapshot, and the current working tree differs from that committed `HEAD` only by a new user follow-up in `exchange`, `commit` must repair the snapshot up to `HEAD` before the no-op `HEAD` comparison. This prevents stale snapshots from staging an older blob and rewinding an already-committed response.
   g. Post-commit local drift classification: if the stripped snapshot already matches `HEAD` but the working tree still differs, `commit` must classify that state as later local drift on top of the committed document before closing as `commit_already_current`. Safe follow-up prompts and arbitrary later working-tree edits both stay uncommitted; the operator-facing explanation must say this is post-commit local drift, not a missed patchback.
   h. Extreme drift guard: when the file is vastly larger than the snapshot, `commit` may auto-resync only for bootstrap scaffold snapshots on files with no `HEAD` entry yet. Tracked documents still do NOT wholesale re-sync from the live file, because that would risk absorbing unanswered user prompts.
   i. Relative-path resolution must prefer an existing cwd-local document before falling back to a superproject root. This prevents submodule sessions like `src/boost-client` from accidentally staging an outer-repo shadow file that happens to share the same relative path (for example `tasks/monsterrodholders.md`).
3. Acquire a blocking advisory commit lock keyed by the resolved git dir (`git rev-parse --absolute-git-dir`), so different docs in the same repo or submodule serialize the short staging+commit critical section instead of racing on one shared index. For submodule documents, workspace-write harness sessions rely on the auto-added external gitdir access from `start` / fresh-agent launch so this lock path and the parent-pointer update remain writable.
4. If no snapshot: fall back to `git add -f <file>` (stages entire file)
5. If the fully staged index already matches `HEAD`, close the cycle as `commit_already_current` and return success without creating a duplicate git commit
6. Otherwise run the full staging+commit transaction. If git reports `index.lock` contention during `update-index`, `git add`, or `git commit`, retry the whole transaction with backoff instead of retrying only the final `git commit` call.
7. On successful commit: keep the on-disk snapshot / visible document in the same clean post-commit shape as the committed blob, then either
   - rewrite the working tree locally to that clean single-boundary shape when no live editor IPC listener exists, or
   - send reposition + VCS-refresh IPC signals so the plugin applies that same clean rewrite via the Document API

**HEAD marker:** `(HEAD)` is transient with respect to the committed blob and snapshot. `agent-doc commit` strips it before staging, and post-commit cleanup strips it from the snapshot. However, `(HEAD)` markers are **preserved in the working tree** (and editor buffer via IPC) so the user sees which response headings are new. Preflight classifies `(HEAD)` differences as `boundary_artifact`, so working-tree `(HEAD)` markers do not cause false-positive change detection.

**Post-commit cleanup:** After a successful commit, the **snapshot** is normalized to the same clean single-boundary shape as the committed blob (no `(HEAD)`, single boundary). The **working tree** is repositioned (stale boundaries removed, fresh boundary inserted) but retains `(HEAD)` annotations on response headings. The IPC reposition signal includes `preserve_head: true` so editor plugins use the `(HEAD)`-preserving FFI variant.

**FFI variants:**
- `agent_doc_reposition_boundary_to_end()` / `_with_id()` — clean variant (strips `(HEAD)`). Used for snapshot cleanup.
- `agent_doc_reposition_boundary_to_end_preserve_head()` / `_with_id()` — preserves `(HEAD)`. Used for working-tree and editor-buffer cleanup.

## skill

`agent-doc skill install` — write the bundled SKILL.md to `.claude/skills/agent-doc/SKILL.md` in the current project. Idempotent (skips if content matches).

`agent-doc skill check` — compare installed skill vs bundled version. Exit 0 if up to date, exit 1 if outdated or missing.

The bundled skill instructions always render `agent-doc-version` from the running binary version at install time. Harness-specific auto-update steps should call `agent-doc skill install --harness <harness>` rather than relying on shell env detection: Claude Code uses `--harness claude --reload compact`, while Codex uses `--harness codex --reload restart`.

## outline

`agent-doc outline <FILE> [--json]` — display markdown section structure with line counts and approximate token counts.

1. Read file, skip YAML frontmatter
2. Parse `#`-prefixed headings into a section tree
3. For each section: heading text, depth, line number, content lines, approximate tokens (bytes/4)
4. Content before the first heading appears as `(preamble)`

Default output: indented text table. `--json` outputs a JSON array of section objects (`heading`, `depth`, `line`, `lines`, `tokens`).

## upgrade

`agent-doc upgrade` — check crates.io for latest version, upgrade via GitHub Releases binary download → cargo install → pip install (cascade).

> **Startup version check:** On every invocation (except `upgrade` itself), `warn_if_outdated` queries crates.io (with a 24h cache at `~/.cache/agent-doc/version-cache.json`) and prints a one-line stderr warning if a newer version is available. Errors are silently ignored so normal operation is never blocked.

## plugin

`agent-doc plugin install <EDITOR>` — download and install the editor plugin from the latest GitHub Release.

`agent-doc plugin update <EDITOR>` — update an installed plugin to the latest version.

`agent-doc plugin list` — list available editor plugins and their install status.

Supported editors: `jetbrains`, `vscode`. Downloads plugin assets from GitHub Releases (`btakita/agent-doc`). Prefers signed assets (`*-signed.zip`) when available, falling back to unsigned. Auto-detects standard plugin directories for each editor (e.g., JetBrains plugin dir via `idea.plugins.path` or platform defaults, VS Code `~/.vscode/extensions/`).

## sync

`agent-doc sync --col <FILES>,... [--col <FILES>,...] [--window W] [--focus FILE]` — declarative 2D layout sync.

Mirrors a columnar editor layout in tmux. Each `--col` is a comma-separated list of files. Columns arrange left-to-right; files stack top-to-bottom within each column.

**Pre-sync file resolution:** Before the layout algorithm runs, sync parses file paths from `--col` args and resolves each file. Files without a session UUID in frontmatter are treated as **unmanaged** and skipped (no auto-initialization of frontmatter). Only `agent-doc claim` adds session UUIDs. Files with session UUIDs are always treated as **registered**, even if the registry entry was pruned (dead pane). This enables the declarative layout flow: navigating to a file in a split creates a tmux pane regardless of registry state. For managed files whose registered pane is in a stash window, sync **rescues** the pane back to the agent-doc window (via `swap-pane`, falling back to `join-pane`) — preserving the existing Claude session context. Only if rescue fails, or if no alive pane exists at all, does sync auto-start a fresh Claude session (via `route::auto_start()`).

**Build stamp:** On each sync invocation, the binary compares its embedded build timestamp (`AGENT_DOC_BUILD_TIMESTAMP` from `build.rs`) against `.agent-doc/build.stamp`. On mismatch (new build detected), all startup locks (`.agent-doc/starting/*.lock`) are cleared and the stamp is updated. This prevents stale locks from old binary instances from blocking auto-start.

**Empty col_args filtering:** Before processing, empty strings in `col_args` are filtered out. The JetBrains plugin sometimes sends phantom empty columns when editor splits change rapidly.

**Column memory:** `.agent-doc/last_layout.json` persists a column→agent-doc mapping across syncs. When a column has no agent doc (user switches to a non-session file), sync substitutes the last known agent doc for that column index. This preserves the 2-pane tmux layout when one editor column temporarily shows a non-agent file. The state file is written after each successful sync for columns that contain an agent doc.

**No early exits:** The full reconcile path always runs regardless of how many panes resolve (0, 1, or 2+). The DETACH phase stashes excess panes from previous layouts. Previous versions had early exits for `resolved < 2` that bypassed stashing, leaving orphaned panes visible.

**Busy pane guard (`layout.rs` only):** The `layout.rs` break_pane path checks `is_pane_busy()` before breaking panes. The sync reconciler's DETACH phase does NOT use a busy pane guard — the `SyncOptions.protect_pane` callback exists in tmux-router but agent-doc passes default options (no guard). This was changed because the guard caused 3-pane accumulation when users switched documents in the same column. Column memory + stash rescue handle session preservation without the guard.

**Reconciliation algorithm** (attach-first order):
1. **SNAPSHOT** — query current pane order in target window
2. **FAST PATH** — if current order matches desired, done
3. **ATTACH** — `join-pane` missing desired panes into target window (isolate from shared windows first, then join with correct split direction: `-h` for columns, `-v` for stacking)
4. **SELECT** — select focus pane before stashing (prevents tmux auto-selecting an unintended pane)
5. **DETACH** — stash unwanted panes out of target window (panes stay alive in stash)
6. **REORDER** — if all panes present but wrong order, break non-first panes out and rejoin in order
7. **VERIFY** — confirm final layout matches desired order

## patch

`agent-doc patch <FILE> <COMPONENT> [CONTENT]` — replace content in a named component.

1. Read the document and parse component markers (`<!-- agent:name -->...<!-- /agent:name -->`)
2. Find the named component (error if not found)
3. Read replacement content from the positional argument or stdin
4. Load component config from `.agent-doc/components.toml` (if present)
5. Apply `pre_patch` hook (stdin: content, stdout: transformed content; receives `COMPONENT` and `FILE` env vars)
6. Apply mode: `replace` (default), `append` (add after existing), or `prepend` (add before existing)
7. If `timestamp` is true, prefix entry with ISO 8601 UTC timestamp
8. If `max_entries > 0` (append/prepend only), trim to last N non-empty lines
9. Write updated document
10. Save snapshot relative to project root
11. Run `post_patch` hook (fire-and-forget; receives `COMPONENT` and `FILE` env vars)

**Component markers:** `<!-- agent:name -->...<!-- /agent:name -->`. Names must match `[a-zA-Z0-9][a-zA-Z0-9-]*`.

**Component config** (`.agent-doc/components.toml`):
```toml
[component-name]
mode = "replace"       # "replace" (default), "append", "prepend"
timestamp = false      # Auto-prefix with ISO timestamp
max_entries = 0        # Trim old entries (0 = unlimited)
max_lines = 0          # Trim to last N lines (0 = unlimited)
pre_patch = "cmd"      # Shell command: stdin→stdout transform
post_patch = "cmd"     # Shell command: fire-and-forget
```

## write

`agent-doc write <FILE> [--baseline-file PATH] [--stream] [--ipc] [--force-disk] [--origin ORIGIN]` — apply patch blocks from stdin to a template document.

1. Read response (patch blocks) from stdin
2. Parse `<!-- patch:name -->...<!-- /patch:name -->` blocks
3. Read document and baseline (from `--baseline-file` or current file)
4. Apply patches to baseline:
   - Mode resolution chain applies normally: inline attribute > `components.toml` > built-in default (`replace`)
   - All components use their resolved mode (no hardcoded overrides for exchange)
5. CRDT merge: if the file was modified during response generation, merge `content_ours` (baseline + patches) with `content_current` (file on disk) using Yrs CRDT
6. Atomic write + snapshot save + CRDT state save

**`--stream` flag:** Enables CRDT write strategy. Required for template/CRDT documents.

**`--ipc` flag:** Writes a JSON patch file to `.agent-doc/patches/` for IDE plugin consumption instead of modifying the document directly.

**`--force-disk` flag:** Bypasses IPC and writes directly to disk, even when `.agent-doc/patches/` exists (plugin installed).

**`--origin` flag:** Write-origin identifier for tracing (e.g., `skill`, `watch`, `stream`). Logged to `ops.log` as `write_origin file=<path> origin=<value>`. Used with the commit drift warning to trace which process wrote to a file.

**IPC-first behavior (v0.17.5):** When `.agent-doc/patches/` exists (plugin installed) and `--force-disk` is not set, IPC is tried first. `try_ipc()` handles component patches; `try_ipc_full_content()` handles full-document replacement (inline mode). Both check for `.agent-doc/patches/` directory existence first — if absent (no plugin active), they return immediately without delay. On IPC timeout (2s), exits with code 75 (`EX_TEMPFAIL`) instead of falling back to disk write. On IPC success, snapshot is saved from `content_ours` (baseline + response), NOT the current file on disk. This ensures user edits typed after the boundary marker are not absorbed into the snapshot and remain visible to the next diff. CRDT state is also saved from `content_ours`.

**Durable response capture (v0.33.13):** Before any write path mutates the document or fires hooks, the final parsed response is persisted to `.agent-doc/captures/<doc-hash>/<cycle-id>.json` with the cycle ID, session/agent/model metadata, response SHA-256, and the exact snapshot/document hashes the response was generated against. The capture ledger also preserves lifecycle provenance (`write_applied_at`, `replayed_at`, `committed_at`, `discarded_at`) so later recovery closeout remains distinguishable from an original same-turn patchback. `pending/<hash>.md` remains the short-lived queue, but it is now a projection of that durable capture rather than the only durable copy.

**Manual-repair contract:** For documented manual repair across Claude Code and Codex, once the user prompt already exists in the document the assistant response path is `agent-doc write --commit <FILE>`. Do not document or rely on a repair flow that stops after bare `agent-doc write`; that leaves the cycle open on the wrong side of the response-commit boundary.

**`--commit` behavior:** `agent-doc write --commit <FILE>` remains a best-effort convenience: it runs the normal write path, then tries `git::commit()`. Outside git it warns and skips commit; inside git it warns on commit failure but still reports the underlying write result. This is the documented manual-repair path because it preserves the older CLI surface while crossing the write/commit boundary in one invocation.

**Response-commit invariant:** Every appended response must cross a commit boundary unless the user explicitly asks to leave it uncommitted. The default happy-path command for normal response cycles is `agent-doc finalize <FILE>`; bare `agent-doc write` is for explicit no-commit exceptions or intermediate checkpoints, not for the final response.

**Write dedup (v0.28.2):** All four write paths skip the actual write when the merged/patched content is identical to the current file on disk. On dedup, pending state is cleared and the function returns early. Events are logged to stderr and appended (with backtrace) to `/tmp/agent-doc-write-dedup.log`.

**Pane ownership verification (v0.28.2):** `verify_pane_ownership()` is called at the top of `run`, `run_template`, and `run_stream`. It reads the document's `session` frontmatter field, looks up the owning pane in the session registry, and compares it to the current tmux pane. If a different pane definitively owns the session, the write is rejected. The check is lenient: it passes silently when not in tmux, when there is no session ID, or when the pane is indeterminate.

**Snapshot invariant:** All write paths (inline, template, stream, IPC) save the snapshot as `content_ours` — the baseline with the agent response applied. The working tree file may differ (due to concurrent user edits merged in), but the snapshot always reflects only the agent's contribution. This is the foundation of correct diff detection.

**Exchange prompt normalization:** For append-mode `agent:exchange`, the write path prefixes newly added user lines with `❯ ` based on the `snapshot -> baseline` diff, but the required prefix targets come from the canonical `prompt_bearing_changes` classifier rather than a separate prompt-shape heuristic. This normalization must ignore synthetic heading-only churn from binary-owned commit markers (for example a response heading gaining ` (HEAD)` in the committed snapshot). A heading replacement may preserve existing agent content, but it must not suppress `❯ ` prefixing for the next genuine prompt-bearing user block.

**Template structure guard:** After patch application and again after any merge, template writes must keep live conversation content inside `<!-- agent:exchange -->`. If a safe trailing `## User` / `## Assistant` / `### Re:` tail escaped below `<!-- /agent:exchange -->`, the binary moves it back inside exchange before snapshot/write. Comment-only notes or other non-conversation scratch content without escaped conversation headings must stay outside `agent:exchange`. If the trailing suffix mixes conversation with other document structure, the write fails closed instead of committing a malformed template document.

**Boundary marker lifecycle (binary-owned):** Boundary management is fully deterministic and handled by the binary — never by the SKILL workflow. The `apply_patches()` function manages the complete lifecycle:

1. **Pre-patch cleanup:** Remove ALL stale boundary markers from the entire document (not just the target component)
2. **Fresh insertion:** Insert a new boundary at the END of the exchange component (after all user text)
3. **Patch application:** Response content is inserted at the boundary position via `append_with_boundary()`
4. **Post-patch re-insertion:** A new boundary is inserted at the END of exchange (after the response)

**Boundary marker format:** `<!-- agent:boundary:{id} -->` where `{id}` is an 8-character hex string (first 8 chars of a UUID v4 with hyphens removed). Short IDs reduce visual noise while maintaining negligible collision probability (~4.3 billion values, self-correcting on collision via next cycle's cleanup).

**Invariants:**
- At most ONE boundary marker exists in the document at any time (outside of code blocks)
- User prompts typed while idle always appear before the response because the fresh boundary is placed after all user text
- The boundary is the dividing line — content before boundary = before response, content after boundary = after response
- Boundaries inside fenced code blocks are excluded from all scanning and cleanup operations

**Cleanup scope:** `remove_all_boundaries()` scans the ENTIRE document (not just the exchange component) and removes every `<!-- agent:boundary:... -->` line that is not inside a fenced code block. This prevents stale boundary accumulation from interrupted cycles or plugin bugs. A single fresh boundary is then inserted at end-of-exchange.

**Design principle:** Boundary insertion was initially implemented in the SKILL workflow (step 1b) but moved to the binary because: (1) it's deterministic (unit-testable with fixed inputs), (2) ALL write paths need it (SKILL, run, stream, watch), (3) non-SKILL paths bypassing step 1b caused stale boundary bugs. **Rule: when adding deterministic operations, ask "will ALL write paths need this?" If yes, it belongs in the binary.**

**IPC boundary:** Before building the IPC patch JSON, all IPC write paths call the clean boundary reposition helper on the current document in memory. This removes stale boundaries and strips transient heading-level ` (HEAD)` markers before inserting a fresh boundary at the end of the exchange. The repositioned document is used only for `boundary_id` extraction (never written to disk by this step). Without this, the IPC path would read the old boundary position (above the user's new prompt), causing responses to be inserted before the prompt. When no explicit patches exist but unmatched content targets `exchange`/`output` and a boundary marker is present, `try_ipc()` synthesizes a boundary-aware exchange patch automatically.

**FFI exports:**
- `agent_doc_reposition_boundary_to_end(doc)` — clean variant. Returns document with stale boundaries removed, `(HEAD)` markers stripped, and a single fresh boundary at end-of-exchange. Used for snapshot cleanup.
- `agent_doc_reposition_boundary_to_end_with_id(doc, id)` — clean variant with explicit boundary ID. Used by post-commit editor refresh.
- `agent_doc_reposition_boundary_to_end_preserve_head(doc)` — preserves `(HEAD)` annotations. Stale boundaries removed, fresh boundary inserted, but `(HEAD)` markers remain. Used for working-tree and editor-buffer post-commit cleanup.
- `agent_doc_reposition_boundary_to_end_preserve_head_with_id(doc, id)` — preserve-head with explicit ID. Editor plugins should call this on post-commit reposition when the IPC signal has `preserve_head: true`.

## finalize

`agent-doc finalize <FILE> [write flags...]` — strict happy-path response write for session documents.

1. Validate that `<FILE>` lives in a git repository before mutating the document.
2. Reuse the same mutation surface as `write`: pending/status mutations are applied first, then the response write path is selected from `--ipc` / `--stream` / `--template` / auto-detect.
3. Run the normal write pipeline (`write`, `run_template`, `run_stream`, or `run_ipc`) with the same snapshot/capture/merge semantics as `agent-doc write`.
4. Invoke `git::commit(<FILE>)` even if the write path returned an error, because the write may have partially succeeded after persisting snapshot/cycle state.
5. Fail unless the final persisted cycle state for the document is `committed`.

**Contract:** `finalize` is the binary-owned happy path for normal session responses. Unlike `write --commit`, it is not best-effort:

- non-git documents are rejected before any write
- `--pending-only` is rejected because `finalize` is for response cycles, not standalone pending maintenance
- success means the cycle closed in `.agent-doc/state/cycles/<hash>.json` as `committed`
- a write error plus a commit error is still a command failure even if some recovery work ran
   - imperative directive diffs (`do #id`, `run tests`, `build + install`, `commit + push`, `go`, or pending-item prose like `[#id] Fix ...`) reject status-only/meta-only responses unless they contain concrete execution evidence or a concrete blocker

## repair

`agent-doc repair <FILE>` (legacy alias: `agent-doc recover`) — repair orphaned pending/captured response state and close git-backed repairs through the normal commit boundary.

1. Run the same recovery engine as `preflight` step 1:
   - replay a pending/captured response when the response still needs to be written
   - dedup and clean stale pending/capture state when the response is already present in the document
   - for template docs, that `AlreadyApplied` dedup path still runs transcript/tail canonicalization before cleanup, including restoring required `❯ ` prompt prefixes from the prompt-bearing classifier
   - respect safe manual removal of an escaped template conversation tail
   - repair a stale `preflight_started` cycle when the persisted snapshot/file hashes still match exactly
2. If recovery work happened and `<FILE>` lives in git, immediately run `agent-doc commit <FILE>`
3. If no pending/captured repair path exists, print the usual "No pending response found" note and stop without committing

**Commit-boundary contract:** For git-backed docs, `repair` must not stop after only updating the live document / pending ledger. A recovered or deduped response should cross the same snapshot+commit boundary in the same command so the next prompt does not inherit repaired-but-uncommitted assistant content. For template docs that means `AlreadyApplied` is still a document-mutation-capable repair outcome when transcript canonicalization is needed.

## watch

`agent-doc watch [--stop] [--status] [--debounce MS] [--max-cycles N]` — watch session files for changes and auto-submit.

- Watches files registered in `sessions.json` for modifications (via `notify` crate)
- On file change (after debounce), runs `submit::run()` on the changed file
- **Reactive mode:** CRDT-mode documents (`agent_doc_write: crdt`) are discovered with `reactive: true` and use zero debounce (`Duration::ZERO`) for instant re-submit on file change. Reactive paths are tracked in a `HashSet<PathBuf>`.
- **Loop prevention:** changes within the debounce window after a submit are treated as agent-triggered; agent-triggered changes increment a cycle counter; if content hash matches previous submit, stop (convergence); hard cap at `--max-cycles` (default 3)
- **Busy guard:** Before submitting, checks `is_busy(file)` via the debounce status signal. If the file has an active agent-doc operation (skill write, stream), the watch daemon skips the file. This prevents the watch daemon from competing with skill writes and causing duplicate responses.
- `--stop` sends SIGTERM to the running daemon (via `.agent-doc/watch.pid`)
- `--status` reports whether the daemon is running
- `--debounce` sets the debounce delay in milliseconds (default 500)

## history

`agent-doc history <FILE>` — list exchange versions from git history.

1. Scan git log for commits touching `<FILE>`
2. Extract the `<!-- agent:exchange -->` component content at each commit
3. Display a list of commits with timestamps and content previews

`agent-doc history <FILE> --restore <COMMIT>` — restore a previous exchange version.

1. Read the exchange content from the specified commit
2. Prepend the old exchange content into the current document's exchange component
3. The restored content appears above the current exchange, preserving both

## transfer

`agent-doc transfer <SOURCE> <TARGET> <COMPONENT> [--bypass-claim] [--items ID1,ID2,...] [--referral]` — move entire component content from source to target document.

1. **Argument validation:** if `--items` is set and component is not `pending`, reject immediately.
2. Validate source exists; auto-create target if missing (template format)
3. **Pane ownership check:** unless `--bypass-claim` is set, verify the current tmux pane owns the target document's session. If a different pane owns it, reject with an error suggesting `--bypass-claim`.
4. **If `--items` is set (selective mode):** match pending lines by `[#id]` pattern, move only matching lines to target, leave the rest in source. Report unmatched IDs as warnings.
5. **Otherwise (full transfer):** read named component from source; bail if empty or absent. Clear source component (single newline). Append content to target's matching component with `> **[TRANSFER from <source>]** (timestamp)` annotation.
6. If the transferred component is not `pending`, also merge pending items from source → target
7. Commit the target so transferred headings appear in git HEAD (prevents `(HEAD)` marking on next cycle)
8. Save snapshots for both files

**`--bypass-claim`:** Explicitly opt into cross-pane transfer. Required when the target document is owned by a different tmux pane. Without it, transfer refuses to write to another pane's document. This flag exists because transfers are deliberate user actions (not concurrent writes) and should not be blocked by session ownership.

**`--items`:** Selective pending transfer. Only moves pending items whose lines contain `[#id]` for each comma-separated ID. IDs may include or omit the `#` prefix (both `--items "#abc,#def"` and `--items "abc,def"` work). Only valid with `component=pending`. Mutually exclusive with `--referral`.

**`--referral`:** Instead of moving content, inserts a structured referral pointer in the target's component:
```html
<!-- agent:referral src="<relative-path>" component="<name>" created="<timestamp>" -->
*Context from [<path>](<path>) — read source <component> for full history.*
<!-- /agent:referral -->
```
The source content stays in place. When preflight sees `<!-- agent:referral -->` tags, it can optionally resolve and inject the referenced content as context. Mutually exclusive with `--items`.

## extract

`agent-doc extract <SOURCE> <TARGET> [--component NAME]` — move the last exchange entry from source to target.

1. Validate both files exist
2. Find last `### Re:` header in named component (default: `exchange`)
3. Split at that position: extracted (last entry) + remaining (everything before)
4. Update source with remaining content
5. Append extracted content to target's matching component with `> **[EXTRACT from <source>]** (timestamp)` annotation
6. Save snapshots for both files

## terminal

`agent-doc terminal <FILE> [--session NAME]` — open an external terminal with tmux attached to the session.

Intended as a fallback for editor plugin commands when no terminal with tmux is open. Prevents duplicate terminal instances by checking for existing attached clients.

1. Resolve tmux session name: `--session` flag > `tmux_session` in document frontmatter > default `"0"`
2. Check if session exists and has an attached client — if so, print message and exit (no-op)
3. If session exists but is detached, open terminal to attach
4. If session does not exist, open terminal which creates and attaches
5. Build tmux command: `tmux new-session -A -s <session>` (attach-or-create)
6. Resolve terminal command (priority order):
   a. `[terminal] command` in `~/.config/agent-doc/config.toml` — template with `{tmux_command}` placeholder
   b. `$TERMINAL` env var — used as `$TERMINAL -e {tmux_command}`
   c. Error with configuration instructions
7. Spawn terminal process (detached)

**Config example:**
```toml
[terminal]
command = "wezterm start -- {tmux_command}"
```

**Safety:** The `{tmux_command}` uses `tmux new-session -A` which attaches to an existing session if it exists, or creates a new one. This means multiple calls to `agent-doc terminal` are idempotent — they either no-op (client already attached) or attach to the existing session.

## preflight

`agent-doc preflight <FILE>` — run all pre-agent steps and output JSON.

Combines interrupted-cycle enforcement, repair, commit, claims-log check, diff, and document HEAD read into a single call. The SKILL workflow consumes the structured JSON output instead of making separate CLI calls.

**Steps (in order):**
0. Enforce previous-cycle completion using persisted per-document cycle state in `.agent-doc/state/cycles/<doc-hash>.json`
   - If the prior cycle is `response_captured` or `write_applied`, preflight first auto-attempts `repair` + `commit`
   - If that `commit` path finds the staged snapshot already matches `HEAD`, it closes the cycle as already committed instead of logging `commit_failed`
   - If the prior cycle is `preflight_started` and the persisted snapshot/file hashes still match exactly, `repair` repairs that stale preflight lock as a no-op closeout
   - Otherwise, preflight still attempts the normal snapshot-only `agent-doc commit <FILE>` closeout before failing. That closeout stages the prior snapshot only, so later live working-tree edits stay uncommitted. If the cycle is still open after `repair` + `commit`, preflight fails closed instead of diffing again
   - If the cycle still has no terminal committed state after that attempt, preflight fails closed instead of silently diffing again
1. Repair orphaned pending/captured responses (`agent-doc repair`, legacy alias: `agent-doc recover`)
   - If a template document's current file matches the captured snapshot except that the user manually removed a safe escaped `## User` / `## Assistant` / `### Re:` tail, `repair` respects that edit: it discards the stale capture, updates the snapshot to the repaired file, and closes the cycle instead of failing hash validation or replaying the removed tail
2. Commit previous cycle (`agent-doc commit`)
3. Read and truncate `.agent-doc/claims.log`
3c. Check linked docs: inspect `links` from frontmatter — local files compared by git commit time, URLs fetched via `ureq` with HTML-to-markdown conversion (htmd), cached in `.agent-doc/links_cache/`
4. Compute diff between snapshot and current document
5. Read document HEAD from disk

**Steps (in order, pre-step 1):**
0. Layout check — `check_layout()` inspects the current tmux session:
   - Check 1: Window 0 exists (base-index compliance)
   - Check 2: Stash windows have no non-idle (running) panes
   - Check 3: Registered panes all belong to the same tmux session (session-drift detection)
   Returns empty outside tmux. Issues are reported in `layout_issues` (informational).

**Output (JSON to stdout):**
```json
{
  "layout_issues": [],
  "recovered": false,
  "committed": true,
  "claims": [],
  "diff": "unified diff text or null",
  "no_changes": false,
  "document": "full document content",
  "slash_commands": ["/clear", "/agent-doc foo.md"],
  "linked_changes": [{"path": "https://example.com", "summary": "content changed (1234 bytes)", "exists": true}]
}
```

- `layout_issues` — array of tmux health warnings (empty = healthy); always present
- `no_changes` is `true` when the diff is `None` (snapshot == document)
- `diff` is `null` when `no_changes` is `true`
- `document` always contains the current HEAD content
- `slash_commands` — slash commands extracted from user-added lines in the diff via `parse_slash_commands()`; omitted when empty. Guards: code fences (``` / ~~~), blockquotes (`>`), non-added lines, and removed lines are excluded. Pattern: `/` followed immediately by an ASCII letter.
- `linked_changes` lists changes in linked docs/URLs since last cycle (omitted when empty)
- Progress/diagnostic messages go to stderr

## session-check

`agent-doc session-check <FILE>` — verify that the previous cycle reached a terminal committed state and that no likely assistant patchback bypassed `agent-doc write` / `finalize`.

- Primary source of truth: `.agent-doc/state/cycles/<doc-hash>.json`
- Fallback for older repos: last non-empty `.agent-doc/logs/ops.log` line
- Exit `1` when the current cycle state is still open (`preflight_started`, `response_captured`, or `write_applied`)
- Exit `1` when the snapshot→file diff contains a likely direct assistant patchback marker such as `### Re:` or `## Assistant` without a corresponding `agent-doc` cycle
- When that bypassed patchback leaves bare `prompt_target` lines in the same changed exchange tail, report the missing-`❯ ` prompt target in the failure marker so repair can route through the binary path instead of silently accepting transcript drift
- Narrow self-heal: if that marker is only historical drift already committed in `HEAD`, and the working tree matches `HEAD` modulo transient boundary / `(HEAD)` markers, `session-check` repairs the stale snapshot first and exits `0`
- Pending-capture guard: after a committed cycle, `session-check` inspects the committed response capture for recommendation-like batches that were not accompanied by any `--pending-add` / `--pending-add-gated` flags in that cycle
- Pending-done guard: after a committed cycle, `session-check` also inspects the committed response capture against still-open `agent:pending` ids and warns when the response appears to complete an existing `#id` task but the cycle recorded no matching `--pending-done <id>`
- Default guard mode is `warn`: emits stderr warnings but still exits `0`
- `pending_capture_guard: strict` in document frontmatter or `.agent-doc/config.toml` `[guards] pending_capture = "strict"` upgrades that condition to exit `1`
- `pending_done_guard: strict` in document frontmatter or `.agent-doc/config.toml` `[guards] pending_done = "strict"` upgrades the missing-`--pending-done` condition to exit `1`
- `pending_capture_guard: off` disables the guard; `<!-- no-pending-capture -->` in the response suppresses it for that cycle (the marker is stripped from the committed blob and from the snapshot/working-tree file post-commit — it is ephemeral signaling only)
- `pending_done_guard: off` disables the missing-`--pending-done` guard; `<!-- no-pending-done-guard -->` in the response suppresses it for that cycle (same strip-after-check behavior as `<!-- no-pending-capture -->`)
- A cycle closed by `agent-doc commit` as `commit_already_current` counts as terminal / committed: it means the staged snapshot was already identical to `HEAD`, so no duplicate git commit was necessary
- Exit `0` when the cycle state is committed or no state/log file exists
- Intended skill/runbook use: the Codex/direct-exec path runs `agent-doc session-check <FILE>` immediately after `agent-doc finalize <FILE> ...` or manual `agent-doc write --commit <FILE> ...`; if the check exits nonzero, the cycle is still open and the agent must fail closed instead of reporting success.

**URL link processing:**
- URLs (`http://`/`https://`) in `links` frontmatter are fetched with a 10s timeout
- HTML responses are converted to markdown via `htmd` (stripping script, style, nav, footer, noscript, svg)
- Content is cached at `.agent-doc/links_cache/<sha256(url)>.txt`
- Changes detected by comparing fresh fetch against cached content

## Preflight Mtime Debounce

The `preflight` command applies a 500ms mtime debounce gate: if the document's filesystem mtime is less than 500ms old, preflight waits until the file has been idle for at least 500ms. This prevents duplicate preflight runs caused by rapid sequential file saves from the editor.

## Unified Diff Context Radius

Diff output now uses a 5-line context radius (unified diff with 5 lines of surrounding context around each hunk). This gives the agent better surrounding context to understand changes.

## Route --debounce

`agent-doc route <FILE> [--debounce MS]` — optional debounce flag to coalesce rapid editor triggers. When set, route will skip execution if another route call for the same file completed within the debounce window.

## is_tracked FFI Export

`agent_doc_is_tracked(path)` — C ABI export for editor plugins. Returns whether the given file path is tracked in `sessions.json` (has a registered session). Plugins use this via JNA/FFI to conditionally show UI elements for tracked documents.

## Sync provision_pane

The sync path uses `provision_pane` instead of the standard auto-start. This variant accepts `col_args: &[String]` and computes `split_before` via `is_first_column(file, col_args)`, so new panes split in the correct direction for their column position (left-column files split before, right-column files split after). It does not block waiting for the `❯` prompt to appear (unlike `route` which waits up to 30s), avoiding sync blocking on slow Claude startup when arranging multiple panes. The call site in `sync.rs` passes the `col_args` slice through from the CLI arguments.

## Sync Swap-Pane Atomic Reconcile

The sync path uses swap-pane atomic transitions via tmux-router. When reconciling pane layout, `provision_pane` spawns sessions without blocking on prompt detection. A `context_session` parameter allows cross-session override — sync knows which session it's managing and passes that context to `auto_start`, which takes priority over the document's `tmux_session` frontmatter field.

## Sync tmux_session Auto-Repair (Deprecated Field)

> **Note:** `tmux_session` in frontmatter is deprecated. This auto-repair mechanism exists for backward compatibility during the deprecation period and will be removed when the field is removed.

When `context_session` (from `sync --window`) differs from the document's `tmux_session` frontmatter value, both `auto_start` and the sync loop automatically repair the frontmatter via direct string replacement. This avoids frontmatter round-trip issues (extra newlines) and ensures the document reflects the actual session assignment after cross-session moves.

## Sync Resync Report-Only

The post-sync `resync` call runs with `--fix` disabled (report only). `auto_start` with `context_session` intentionally places panes in a different session than the frontmatter originally specified — `resync --fix` would incorrectly kill these cross-session panes. The resync still reports anomalies for operator awareness.

## Sync Visible-Window Split

When the sync path (`skip_wait=true`) creates new panes, it prefers splitting in the visible `agent-doc` window of the target session rather than falling back to any registered pane (which may be in a stash window). This ensures new panes appear where the user can see them. Falls back to `find_registered_pane_in_session` if no panes exist in the agent-doc window.

## Repair Layout

`repair_layout` normalizes the tmux window layout before every sync. It receives the tmux handle, session name, and target window name (always `"agent-doc"`). The plugin always passes `--window agent-doc` as a fallback so the target window name is known.

**Phase 1 — Stash consolidation:** Merges all secondary stash windows (`stash-*` and duplicate `stash` windows) into a single primary stash window. For each secondary, all panes are joined into the primary via `join-pane -dv`, targeting the largest pane to avoid "pane too small" errors. Empty secondary windows are killed after pane migration.

**Phase 2 — Window rescue:** If the target `agent-doc` window does not exist, attempts to recreate it by finding an alive registered pane in the stash (via `sessions::load()`), breaking it out with `break-pane`, and renaming the resulting window to `agent-doc`.

**Phase 3 — Index normalization:** Re-lists windows after Phases 1+2 and moves the `agent-doc` window to index 0 via `move-window` if it is not already there. This phase always runs.

**Fast path:** When the target window already exists and there is at most one stash window, Phases 1 and 2 are skipped entirely. Only Phase 3 (index normalization) executes, making the common case a lightweight check.

## session

`agent-doc session` — show the configured tmux session.
`agent-doc session set <name>` — update config.toml and migrate panes to the new session.

**Show:** Reads `.agent-doc/config.toml` `tmux_session` field and prints it (or "(none)").

**Set:** Updates config.toml, then moves the `agent-doc` window and `stash` window from the old session to the new one via `tmux move-window`. If the move fails (target session doesn't exist), config is still updated — subsequent route/claim operations will target the new session.

**Session resolution (`resolve_target_session`):** Single function in route.rs that all session-targeting code paths use. Priority: (1) context_session from sync --window, (2) config.toml if alive, (3) fallback to current session. Config is auto-updated only when the configured session is dead.

## dedupe

`agent-doc dedupe <FILE>` — remove consecutive duplicate response blocks.

Detects consecutive `### Re:` blocks with identical content (after stripping boundary markers) and removes the duplicate. Updates the snapshot after removal. Idempotent — running twice produces the same result.

After removing duplicates and updating the snapshot, `dedupe` also deletes the corresponding stale patch file at `.agent-doc/patches/<hash>.json` (if present). Without this cleanup, `processPendingPatches()` on plugin restart would re-apply the removed content, creating another duplicate.

## plan

`agent-doc plan <FILE>` — derive a structured post-preflight planning/dispatch record for the current document.

The command computes the current diff against the saved snapshot and emits JSON with:

- `prompt_targets`
- `repo_actions`
- `required_commands`
- `pending_mutations`
- `handoff`
- `blockers`

The intent is to give the skill/orchestrator an explicit, binary-owned execution contract before repo work starts. The first implementation is deterministic and reuses the same diff classifiers that back preflight (`prompt_bearing_changes`, imperative directives, slash-command parsing, orchestration detection) rather than hiding planning in skill prose.

## orchestrate

`agent-doc orchestrate <FILE> --mode sequential|parallel|dag [--task TEXT ...] [--from-file TASKS.md] [--from-exchange] [--agent NAME] [--model MODEL] [--dry-run] [--plan]`

**Skill-side dispatch:** the bundled skill/runbook treats natural-language orchestration requests as aliases for this command. Ordered phrases like `run these in order`, `chain these`, `one by one`, and `orchestrate` map to `--mode sequential`; concurrency phrases like `fan out`, `concurrent`, and `simultaneously` map to `--mode parallel`; dependency phrases like `after #a do #b`, `depends on`, and `fan in` map to `--mode dag`.

**Compound single-line directives:** the bundled skill/runbook may also normalize one prose task with distinct follow-up clauses into explicit orchestrate steps before invoking this command. Example: `do #ntoc. Add to today's news. commit + push` can be steered into primary work plus explicit follow-up news/update/push steps. This remains skill-side steering rather than binary-owned free-form parsing; the CLI still expects explicit `--task` entries or explicit DAG metadata.

Shared task-source resolution:

1. Collect tasks from repeated `--task` flags, preserving order.
2. If `--from-file` is provided, read the file and extract tasks from the last fenced code block or markdown list that contains list items; otherwise fall back to non-empty trimmed lines.
3. If `--from-exchange` is provided, read the document's `agent:exchange` component. Before extracting tasks, scope the exchange text to only the user's latest additions by comparing against the snapshot — lines matching the snapshot (modulo `(HEAD)` markers and boundary comments) are excluded, and only the newly-added tail is searched. This prevents stale task lists in prior response content from being selected when the user's directive is a bare line at the exchange tail. Falls back to the full exchange text when no snapshot exists. Then apply the same task extraction rule to the scoped text. List items with the `❯ ` prompt prefix (added by the binary write path for user prompts) are normalized by stripping the prefix before parsing, so `❯ - do #task` is treated identically to `- do #task`.
4. Independently scan `--from-file` / `--from-exchange` text for batch-level `preset <name>` or `presets <a>, <b>` directives. These are not tasks; they request frontmatter `prompt_presets` by name. Preserve request order and de-duplicate repeated names.
5. Validate requested presets against the document frontmatter `prompt_presets` map. Missing preset references fail closed.
6. Concatenate all resolved tasks in source order. Error if the final task list is empty.

### `--dry-run` and `--plan`

Both flags exit without executing any agent tasks.

- `--dry-run`: prints task labels only, before preset expansion. Stops after the initial task-list summary.
- `--plan`: resolves tasks fully (including preset expansion via `apply_prompt_preset_block`), then prints each task's fully expanded prompt. Use this to verify what the agent will receive before committing to execution:

```
[orchestrate] plan — 2 task(s) (no execution)
[orchestrate] step 1/2: do #prep
[orchestrate] --- prompt ---
[orchestrate] (preset #spec-test-commit-push)
[orchestrate] update spec + tests. commit + push
[orchestrate] do #prep
[orchestrate] --- end prompt ---
[orchestrate] step 2/2: do #report
...
```

`--plan` is the recommended harness verification step before issuing a live `orchestrate` call. It applies to all three modes (`sequential`, `parallel`, `dag`).

### Frontmatter args forwarding

Orchestrate subprocesses inherit permission/sandbox settings from the document frontmatter using the same precedence chain as `agent-doc start`:

- **Claude:** `fm.agent_args` > `fm.claude_args` > `config.agent_args` > `config.claude_args`
- **Codex:** `fm.agent_args` > `fm.codex_args` > `config.agent_args` > `config.codex_args`

When resolved args exist, the subprocess command is built from structural base args (Claude: `-p --output-format json`; Codex: `exec --json`) plus the resolved frontmatter/config args. When no args are resolved, default base args apply (Claude: `--permission-mode acceptEdits`; Codex: `-s workspace-write`).

### Subprocess stderr surfacing

Streaming agent subprocesses (`send_streaming` in both Claude and Codex backends) drain stderr in a background thread. When the subprocess exits non-zero, the iterator yields a final `Err` containing the exit status and stderr content — e.g. `"claude subprocess exited with exit status: 1: permission denied"`. This replaces the previous behavior where stderr was silently discarded and failures surfaced only as `"empty response from streaming orchestrate step"` with no diagnostics.

When the subprocess exits successfully but stderr is non-empty (warnings, deprecation notices), the content is logged to the parent's stderr with an `[agent]` prefix and does not produce an error.

### Streaming args construction

`build_streaming_args()` transforms the base args for streaming: it strips any existing `--output-format` value and replaces it with `--output-format stream-json`, then unconditionally adds `--verbose`. The `--verbose` flag is required by the Claude CLI when `-p` and `--output-format stream-json` are combined.

### `--mode sequential`

Sequential orchestration runs one full fresh-agent lifecycle per task:

1. Inject `❯ <task>` into `agent:exchange`, immediately before the current boundary marker when one exists.
2. Run `agent-doc preflight <FILE>` after each prompt injection and use its `baseline_file` for response persistence.
3. If that preflight encounters an already-open `preflight_started` cycle from an outer skill/router pass, it first auto-attempts the safe snapshot-only `commit_already_current` closeout for that prior cycle, then continues into the new preflight. Sequential orchestration does not keep a bespoke task-1 reuse path anymore; `preflight` itself is the idempotent boundary.
4. If batch-level prompt presets were requested, prefix the concrete task prompt with one labeled block per preset, for example:

```text
(preset #1)
Today is 2026-04-25.
Keep the work tree clean.

do #prep
```

5. Build the normal edited-document prompt shape (`<diff>` + full `<document>`) from the current document state.
6. Resolve the agent backend from `--agent` → frontmatter `agent` → global config `default_agent` → `"claude"`.
7. For CRDT session docs with an `agent:exchange` component, if the chosen backend supports streaming (`claude` or `codex`), stream the in-progress step response into `exchange` before the normal write boundary. The streamed buffer is "existing exchange through injected prompt" plus the child response so far, with the boundary marker kept at the end.
8. Send exactly one fresh backend request with **no resume** (`session_id=None`, `fork=false`) so steps do not accumulate agent conversation state. When streaming is unavailable, sequential mode falls back to the blocking request path.
9. Persist the final response through `agent-doc finalize <FILE> --baseline-file ...` using the document's resolved write mode (`--stream` for CRDT docs, `--template` for merge/template docs, inline for append docs).
10. Run `agent-doc session-check <FILE>` immediately after finalize. Stop on the first failure.

Notes:

- Sequential orchestration requires the normal git-backed finalize path; `--no-git` is rejected.
- The imperative-response contract still applies because persistence flows through `finalize`.
- The injected prompt becomes part of the document/system-of-record before the agent runs.
- The same `preflight` call shape is used for every task. Any inherited open-cycle cleanup happens inside `preflight` before the new diff is emitted.
- Streamed step patchback is provisional until the final `finalize -> session-check` closeout succeeds. The stream path improves document visibility; the binary-owned commit boundary is still the authoritative persistence step.

### `--mode parallel`

Parallel orchestration resolves tasks through the shared orchestrate surface, then runs the existing worktree fan-out backend:

- `--task` / `--from-file` / `--from-exchange` are resolved first.
- Any requested prompt presets are expanded into the concrete per-task prompt before the parallel worker prompt file is written, while the user-facing task label stays unchanged.
- The resolved tasks are then passed to the existing parallel engine with `--model`, `--no-git`, `--no-worktree`, and `--timeout`.
- The legacy `agent-doc parallel` command is a compatibility wrapper over this same dispatch path with explicit `--task` entries only, so both surfaces share task normalization and mode routing.

### `--mode dag`

Dependency-aware orchestration against the same shared document lifecycle.

Task entries may include an optional metadata prefix before the real prompt:

```md
- do #prep. Prepare context
- [after=#prep] do #bench. Run benchmarks
- [id=report after=#prep,#bench] Summarize both results
```

Rules:

1. If metadata provides `id=...`, that becomes the node id.
2. Otherwise the first `#token` found in the prompt becomes the node id (for example `do #prep...` → `#prep`).
3. If neither exists, the node gets an implicit `step-N` id.
4. `after=` (alias `deps=`) lists comma-separated prerequisite ids.
5. Unknown dependencies, duplicate ids, and dependency cycles fail fast before any task executes.

Execution semantics:

- The graph is topologically sorted in deterministic source order.
- Each ready node still runs through the normal single-document lifecycle: inject prompt → `preflight` → fresh agent request → `finalize` → `session-check`.
- Requested prompt presets are expanded into each DAG node's concrete prompt before dispatch, so `sequential`, `dag`, and `parallel` share the same labeled preset block behavior.
- Because every node writes back to the same session document, DAG mode does **not** run siblings concurrently. Fan-in is supported through multiple `after=` dependencies; fan-out across isolated worktrees remains `--mode parallel`.
- `--no-git` is rejected for the same reason as sequential mode: persistence still flows through git-backed `finalize`.
