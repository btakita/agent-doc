# agent-doc Functional Specification

> Language-independent specification for the agent-doc interactive document session tool.
> This document captures the exact behavior a port must reproduce.

## 1. Overview

agent-doc manages interactive document sessions between a human and an AI agent.
The human edits a markdown document, sends diffs to the agent, and the agent's
response is appended. Session state is tracked via YAML frontmatter, snapshots,
and git commits.

## 2. Document Format

### 2.1 Session Document

Frontmatter fields:
- `agent_doc_session`: Document/routing UUID — permanent identifier for tmux pane routing. Legacy alias: `session` (read but not written).
- `agent_doc_format`: Document format — `inline` (canonical), `template` (default: `template`). `append` accepted as backward-compat alias for `inline`.
- `agent_doc_write`: Write strategy — `merge` or `crdt` (default: `crdt`).
- `agent_doc_mode`: **Deprecated.** Single field mapping: `append` → format=append, `template` → format=template, `stream` → format=template+write=crdt. Explicit `agent_doc_format`/`agent_doc_write` take precedence. Legacy aliases: `mode`, `response_mode`.
- `agent`: Agent backend name (overrides config default)
- `model`: Model override (passed to agent backend)
- `branch`: Reserved for branch tracking
- `claude_args`: Additional CLI arguments for the `claude` process (space-separated string, see §6.1)

All fields are optional and default to null. Resolution: explicit `agent_doc_format`/`agent_doc_write` > deprecated `agent_doc_mode` > defaults (template + crdt). The body alternates `## User` and `## Assistant` blocks (append format) or uses named components (template format).

### 2.2 Frontmatter Parsing

Delimited by `---\n` at file start and closing `\n---\n`. If absent, all fields default to null and entire content is the body.

### 2.3 Components

Documents can contain named, re-renderable regions called components:

```html
<!-- agent:status -->
content here
<!-- /agent:status -->
```

Marker format: `<!-- agent:{name} -->` (open) and `<!-- /agent:{name} -->` (close). Names must match `[a-zA-Z0-9][a-zA-Z0-9-]*`. Components are patched via `agent-doc patch`.

**Inline attributes:** Open markers support inline attribute overrides: `<!-- agent:name patch=append -->`. `mode=` is accepted as a backward-compatible alias; `patch=` takes precedence if both are present. `max_lines=N` trims component content to the last N lines after patching (0 or absent = unlimited). Precedence chain: inline attribute > `.agent-doc/components.toml` > built-in default (`replace` for patch, unlimited for max_lines).

**Code range exclusion:** Component marker detection uses pulldown-cmark for CommonMark-compliant code range detection, replacing the previous regex-based approach. Markers inside inline code spans or fenced code blocks are excluded and never treated as component boundaries.

**Standard component names:**

| Component | Default `patch` | Description |
|-----------|----------------|-------------|
| `exchange` | append | Conversation history — each cycle appends |
| `findings` | append | Accumulated research data — grows over time |
| `status` | replace | Current state — updated at milestones |
| `pending` | replace | Task queue — auto-cleaned each cycle |
| `output` | replace | Latest agent response only |
| `input` | replace | User prompt area |
| (custom) | replace | All other components default to replace |

Per-component behavior is configured in `.agent-doc/components.toml` (see §7.21).

## 3. Snapshot System

### 3.1 Storage

Snapshots live in `.agent-doc/snapshots/` relative to CWD. Path: `sha256(canonical_path) + ".md"`.

### 3.2 Lifecycle

- **Save**: After successful run, full content saved as snapshot
- **Load**: On next run, loaded as "previous" state for diff
- **Delete**: On `reset`, snapshot removed
- **Missing**: Diff treats previous as empty (entire doc is the diff)

## 4. Diff Computation

Line-level unified diff via `similar` crate. Returns `+`/`-`/` ` prefixed lines, or None if unchanged.

> **Skill-level behavior:** The `/agent-doc` Claude Code skill strips HTML comments (`<!-- ... -->`) and link reference comments (`[//]: # (...)`) from both the snapshot and current content before diff comparison. This ensures that comments serve as a user scratchpad without triggering agent responses. This stripping is performed by the skill workflow (SKILL.md §2), not by the CLI itself.

## 5. Agent Backend

### 5.1 Trait

`fn send(prompt, session_id, fork, model) -> (text, session_id)`

### 5.2 Resolution Order

1. CLI `--agent` flag
2. Frontmatter `agent` field
3. Config `default_agent`
4. Fallback: `"claude"`

### 5.3 Claude Backend

Default: `claude -p --output-format json --permission-mode acceptEdits`. Session handling: `--resume {id}` or `--continue --fork-session`. Appends `--append-system-prompt` with document-mode instructions. Removes `CLAUDECODE` env var. Parses JSON: `result`, `session_id`, `is_error`.

### 5.4 Custom Backends

Config overrides `command` and `args` for any agent name.

## 6. Config

Location: `{XDG_CONFIG_HOME}/agent-doc/config.toml` (default `~/.config/agent-doc/config.toml`).

Fields: `default_agent`, `claude_args`, `[agents.{name}]` with `command`, `args`, `result_path` (reserved), `session_path` (reserved).

### 6.1 claude_args

Additional CLI arguments passed to the `claude` process when spawned by `agent-doc start`. Space-separated string.

Three sources, in precedence order (highest first):

1. **Frontmatter**: `claude_args: "--dangerously-skip-permissions"` in the document's YAML frontmatter
2. **Global config**: `claude_args = "--dangerously-skip-permissions"` in `~/.config/agent-doc/config.toml`
3. **Environment variable**: `AGENT_DOC_CLAUDE_ARGS="--dangerously-skip-permissions"`

The resolved args are split on whitespace and prepended to the `claude` command before other flags (e.g., `--continue`).

### 6.2 Project Config

Location: `.agent-doc/config.toml` (relative to project root).

Fields: `tmux_session` — the tmux session name bound to this project.

**Auto-sync:** When the configured `tmux_session` is dead (session no longer exists), the route path falls back to `current_tmux_session()` and auto-updates `config.toml` with the new session name. This prevents stale config after session destruction.

### 6.3 Socket IPC

Socket-based IPC via Unix domain sockets (`.agent-doc/ipc.sock`) is the primary IPC transport. The editor plugin starts a listener via `agent_doc_start_ipc_listener()` FFI call on project open. The CLI sender connects, sends NDJSON messages, and waits for ack.

**Protocol:** Newline-delimited JSON (NDJSON). Message types:
- `{"type": "patch", "file": "...", "patches": [...], ...}` — apply component patches
- `{"type": "reposition", "file": "..."}` — reposition boundary marker
- `{"type": "vcs_refresh"}` — trigger VCS/VFS refresh

**Fallback:** If socket is unavailable (no listener), falls back to file-based IPC (JSON patch files in `.agent-doc/patches/`).

### 6.4 IPC Write Verification

After the IDE plugin consumes an IPC patch file:
1. **File-change check:** If the document file is unchanged on disk, the plugin failed to apply — falls back to disk write.
2. **Content verification:** If the document changed but none of the patch content appears in the result, the plugin partially failed — falls back to disk write.
3. **Force-disk cleanup:** When `--force-disk` is set, any pending IPC patch files are deleted before disk write to prevent the plugin from applying stale patches (double-write prevention).

### 6.5 Sync Layout Authority

`sync_after_claim()` uses editor-provided `col_args` when available (authoritative layout from the IDE plugin). Only falls back to registry-based file discovery when no `col_args` given. This prevents stale registry entries from creating incorrect multi-pane layouts.

### 6.6 Document State Model (4 States)

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

## 7. Commands

### 7.1 run

`agent-doc run <FILE> [-b] [--agent NAME] [--model MODEL] [--dry-run] [--no-git]`

1. Compute diff → 2. Build prompt (diff + full doc) → 3. Branch if `-b` → 4. Send to agent → 5. Update session ID → 6. Append response → 7. Save snapshot → 8. `git add -f` + commit

First run prompt wraps full doc in `<document>` tags. Subsequent wraps diff in `<diff>` tags + full doc in `<document>`.

### 7.2 init

Two modes:

**No-arg (project init):** `agent-doc init` — checks prerequisites, creates `.agent-doc/snapshots/` and `.agent-doc/patches/` directories, and installs `.claude/skills/agent-doc/SKILL.md`. Idempotent. Run once per project before creating session documents.

**With file (document scaffold):** `agent-doc init <FILE> [TITLE] [--agent NAME]` — scaffolds frontmatter + `## User` block. Fails if file already exists. Lazily runs project init first if `.agent-doc/` does not exist.

### 7.3 install

`agent-doc install [--editor jetbrains|vscode] [--skip-prereqs] [--skip-plugins]` — system-level setup.

1. **Prerequisite check** (unless `--skip-prereqs`): verifies `tmux` and `claude` are on `PATH`; prints ok or MISSING with install hint for each. Does not fail — only warns.
2. **Editor plugin install** (unless `--skip-plugins`):
   - If `--editor` is given, installs only that editor's plugin.
   - Otherwise, auto-detects installed editors: JetBrains (checks `~/.local/share/JetBrains/` on Linux, `/Applications/IntelliJ*` on macOS) and VS Code family (`cursor`, `codium`, `code`).
   - If no editors detected, prints a hint to use `--editor` and exits without error.
   - Calls `crate::plugin::install(editor)` for each detected editor.
   - Prints a summary of installed and failed editors.

### 7.4 diff

`agent-doc diff <FILE>` — prints unified diff to stdout.

### 7.5 reset

`agent-doc reset <FILE>` — clears session ID, deletes snapshot.

### 7.6 clean

`agent-doc clean <FILE>` — squashes all `agent-doc:` commits for file into one via `git reset --soft`.

### 7.7 audit-docs

`agent-doc audit-docs [--root DIR]` — checks CLAUDE.md/AGENTS.md/README.md/SKILL.md for tree path accuracy, line budget (1000), staleness, and actionable content. Exit 1 on issues.

`--root DIR` overrides auto-detection of the project root directory. Without it, the root is resolved via project markers (Cargo.toml, package.json, etc.), then `.git`, then CWD fallback.

### 7.8 start

`agent-doc start <FILE>` — start Claude in a new tmux pane and register the session.

1. Ensure session UUID in frontmatter (generate if missing)
2. Read `$TMUX_PANE` (must be inside tmux)
3. Register session → pane in `sessions.json`
4. Exec `claude` (replaces process)

### 7.9 route

`agent-doc route <FILE> [--pane P]` — route a `/agent-doc` command to the correct tmux pane.

1. Prune stale entries from `sessions.json`
2. Ensure session UUID in frontmatter (generate if missing)
3. Look up pane in `sessions.json`
4. If pane alive → send `/agent-doc <FILE>` via `send_keys`, then Enter verification loop (polls for command text disappearance every 300ms, retries Enter on each poll, up to 5s timeout), focus pane
5. If pane dead (previously registered) → lazy-claim to active pane in `claude` tmux session (or `--pane P`), register, send command, auto-sync layout for all files in the same window. Unregistered files skip lazy-claim entirely.
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

### 7.10 claim

`agent-doc claim <FILE> [--position left|right|top|bottom] [--window W] [--pane P]` — claim a document for a tmux pane.

1. Ensure session UUID in frontmatter (generate if missing)
2. **Resolve effective window** (see Window Resolution below)
3. Determine pane: `--pane P` overrides, else `--position` resolves via tmux pane geometry, else `$TMUX_PANE`
4. Register session → pane in `sessions.json`, including window ID

Unlike `start`, does not launch Claude — the caller is already inside a Claude session. `--position` is used by the JetBrains plugin to map editor split positions to tmux panes.

**Registry protection:** If the target pane is already claimed by a different session (and the pane is alive), `claim` refuses with an error. Use `--force` to overwrite an existing claim. This prevents silent corruption when position detection falls back to the wrong pane.

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

### 7.11 focus

`agent-doc focus <FILE> [--pane P]` — focus the tmux pane for a session document.

1. Read session UUID from file's YAML frontmatter (or use `--pane` override)
2. Look up pane ID in `sessions.json`
3. Run `tmux select-window -t <pane-id>` then `tmux select-pane -t <pane-id>`

Exits with error if the pane is dead or no session is registered.

### 7.12 layout

`agent-doc layout <FILE>... [--split h|v] [--window W]` — arrange tmux panes to mirror editor split layout.

1. Resolve each file to its session pane via frontmatter → `sessions.json`
2. If `--window` given, filter to panes registered for that window only
3. Pick the target window (the one containing the most wanted panes; tiebreak: most total panes)
4. Break out only registered session panes that aren't wanted (shells and tool panes are left untouched)
5. Join remaining wanted panes into the target window (`tmux join-pane`)
6. Focus the first file's pane (the most recently selected file)

`--split h` (default): horizontal/side-by-side. `--split v`: vertical/stacked. Single file falls back to `focus`. Dead panes and files without sessions are skipped with warnings.

### 7.13 resync

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
- Wrong-session panes: kills the pane via `tmux kill-pane`, removes registry entry. Next `route` auto-starts in the correct session.
- Wrong-process panes: removes registry entry only (does not kill the foreign process). Next `route` auto-starts a new pane.
- Wrong-window panes: moves the pane into the stash window via `stash_pane` (does not deregister). The pane stays alive; the next `sync` or `layout` rejoins it into the correct window.

**Stash window naming:** Stash windows are named `stash`. When tmux auto-deduplicates a name collision the window becomes `stash-2`, `stash-3`, etc. All names matching `stash` or `stash-*` are treated as stash windows (checked by `is_stash_window_name`). `resync` purges stash windows where all panes are idle shells and last activity was >30s ago.

**Auto-start stash overflow (route):** When `auto_start_in_session` tries `split-window` alongside a registered pane and the split fails (e.g. minimum pane size constraint), it falls back to `tmux new-window` then immediately calls `stash_pane` to move the new pane into the stash window — avoiding a visible throwaway window in the session.

**Automatic pruning:** `resync::prune()` (step 1 only — no issue detection or fixing) runs automatically before `route`, `sync`, and `claim` operations. Uses bulk metadata fetching (2 subprocess calls: `list-windows -a` + `list-panes -a`) instead of per-pane queries. Stranded panes (no valid return target) are deregistered on first failure to prevent repeated expensive lookups. **Stash pane safety:** unregistered agent processes (`agent-doc`, `claude`, `node`) in stash windows are never auto-killed — only idle shells are purged. This prevents loss of active Claude sessions when the registry goes stale.

### 7.14 prompt

`agent-doc prompt <FILE>` — detect permission prompts from a Claude Code session.

- Captures tmux pane content, strips ANSI, searches bottom-up for footer containing `"to cancel"`
- Supports two option formats: bracket `[N] label` (legacy) and numbered list `N. label` (Claude Code v2.1+)
- Returns JSON: `{ "active": bool, "question": str, "options": [...], "selected": int }`
- `--answer N` navigates to option N via arrow keys and confirms with Enter
- `--all` polls all live sessions, returns JSON array of `PromptAllEntry` objects
- Debug: `AGENT_DOC_PROMPT_DEBUG=1` logs last 5 non-empty lines of each captured pane to stderr

### 7.15 commit

`agent-doc commit <FILE>` — selective commit with auto-generated timestamp.

1. Load the snapshot for the file (the document state after the last `agent-doc write`)
2. If snapshot exists:
   a. Add `(HEAD)` suffix to all new markdown headings (any level `#`–`######`) not present in git HEAD. Falls back to bold-text pseudo-headers (`**...**` on its own line) when no markdown headings are found.
   b. Write the modified snapshot to git's object database via `git hash-object -w --stdin`
   c. Stage via `git update-index --add --cacheinfo 100644,<hash>,<file>` — working tree is NOT modified
   d. Result: snapshot content (agent response) is committed; user edits in the working tree stay uncommitted
3. If no snapshot: fall back to `git add -f <file>` (stages entire file)
4. `git commit -m "agent-doc(<stem>): <timestamp>" --no-verify`
5. On successful commit: write `vcs-refresh.signal` to `.agent-doc/patches/` — the IDE plugin watches this and triggers `VcsDirtyScopeManager.markEverythingDirty()` + VFS refresh so git gutter updates immediately

**HEAD marker:** The committed version has ` (HEAD)` appended to new root-level headings. When no markdown headings exist, bold-text pseudo-headers (`**...**` on its own line) receive the marker instead. The working tree does not have these markers. This creates a single modified-line gutter (blue) at each heading — a visual boundary between committed agent response and uncommitted user input.

**Duplicate heading detection:** Headings are identified as "new" by comparing occurrence counts between the current content and git HEAD. A heading is new if it appears more times in the current content than in HEAD. This correctly handles duplicate heading text across exchange cycles (e.g., multiple `### Re: Implementation complete` headings from different responses).

**Post-commit cleanup:** After a successful commit, `(HEAD)` markers are stripped from headings and bold-text pseudo-headers in both the snapshot and the working tree file. This prevents stale markers from accumulating across commits.

### 7.16 skill

`agent-doc skill install` — write the bundled SKILL.md to `.claude/skills/agent-doc/SKILL.md` in the current project. Idempotent (skips if content matches).

`agent-doc skill check` — compare installed skill vs bundled version. Exit 0 if up to date, exit 1 if outdated or missing.

The bundled SKILL.md contains an `agent-doc-version` frontmatter field set to the binary's version at build time. When the skill is invoked via Claude Code, the pre-flight step compares this field against the installed binary version (`agent-doc --version`). If the binary is newer, `agent-doc skill install` runs automatically to update the skill before proceeding.

### 7.17 outline

`agent-doc outline <FILE> [--json]` — display markdown section structure with line counts and approximate token counts.

1. Read file, skip YAML frontmatter
2. Parse `#`-prefixed headings into a section tree
3. For each section: heading text, depth, line number, content lines, approximate tokens (bytes/4)
4. Content before the first heading appears as `(preamble)`

Default output: indented text table. `--json` outputs a JSON array of section objects (`heading`, `depth`, `line`, `lines`, `tokens`).

### 7.18 upgrade

`agent-doc upgrade` — check crates.io for latest version, upgrade via GitHub Releases binary download → cargo install → pip install (cascade).

> **Startup version check:** On every invocation (except `upgrade` itself), `warn_if_outdated` queries crates.io (with a 24h cache at `~/.cache/agent-doc/version-cache.json`) and prints a one-line stderr warning if a newer version is available. Errors are silently ignored so normal operation is never blocked.

### 7.19 plugin

`agent-doc plugin install <EDITOR>` — download and install the editor plugin from the latest GitHub Release.

`agent-doc plugin update <EDITOR>` — update an installed plugin to the latest version.

`agent-doc plugin list` — list available editor plugins and their install status.

Supported editors: `jetbrains`, `vscode`. Downloads plugin assets from GitHub Releases (`btakita/agent-doc`). Prefers signed assets (`*-signed.zip`) when available, falling back to unsigned. Auto-detects standard plugin directories for each editor (e.g., JetBrains plugin dir via `idea.plugins.path` or platform defaults, VS Code `~/.vscode/extensions/`).

### 7.20 sync

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

### 7.21 patch

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

### 7.22 write

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

**Write dedup (v0.28.2):** All four write paths skip the actual write when the merged/patched content is identical to the current file on disk. On dedup, pending state is cleared and the function returns early. Events are logged to stderr and appended (with backtrace) to `/tmp/agent-doc-write-dedup.log`.

**Pane ownership verification (v0.28.2):** `verify_pane_ownership()` is called at the top of `run`, `run_template`, and `run_stream`. It reads the document's `session` frontmatter field, looks up the owning pane in the session registry, and compares it to the current tmux pane. If a different pane definitively owns the session, the write is rejected. The check is lenient: it passes silently when not in tmux, when there is no session ID, or when the pane is indeterminate.

**Snapshot invariant:** All write paths (inline, template, stream, IPC) save the snapshot as `content_ours` — the baseline with the agent response applied. The working tree file may differ (due to concurrent user edits merged in), but the snapshot always reflects only the agent's contribution. This is the foundation of correct diff detection.

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

**IPC boundary:** Before building the IPC patch JSON, all IPC write paths call `reposition_boundary_to_end()` on the current document in memory. This removes stale boundaries and inserts a fresh one at the end of the exchange — the same pre-patch step that `apply_patches_with_overrides()` performs. The repositioned document is used only for `boundary_id` extraction (never written to disk by this step). Without this, the IPC path would read the old boundary position (above the user's new prompt), causing responses to be inserted before the prompt. When no explicit patches exist but unmatched content targets `exchange`/`output` and a boundary marker is present, `try_ipc()` synthesizes a boundary-aware exchange patch automatically.

**FFI export:** `agent_doc_reposition_boundary_to_end(doc)` — exposed via C ABI for editor plugins. Takes a document string, returns a cleaned document with all stale boundaries removed and a single fresh 8-char boundary at end-of-exchange. Plugins should call this via JNA/FFI rather than reimplementing boundary cleanup logic.

### 7.23 watch

`agent-doc watch [--stop] [--status] [--debounce MS] [--max-cycles N]` — watch session files for changes and auto-submit.

- Watches files registered in `sessions.json` for modifications (via `notify` crate)
- On file change (after debounce), runs `submit::run()` on the changed file
- **Reactive mode:** CRDT-mode documents (`agent_doc_write: crdt`) are discovered with `reactive: true` and use zero debounce (`Duration::ZERO`) for instant re-submit on file change. Reactive paths are tracked in a `HashSet<PathBuf>`.
- **Loop prevention:** changes within the debounce window after a submit are treated as agent-triggered; agent-triggered changes increment a cycle counter; if content hash matches previous submit, stop (convergence); hard cap at `--max-cycles` (default 3)
- **Busy guard:** Before submitting, checks `is_busy(file)` via the debounce status signal. If the file has an active agent-doc operation (skill write, stream), the watch daemon skips the file. This prevents the watch daemon from competing with skill writes and causing duplicate responses.
- `--stop` sends SIGTERM to the running daemon (via `.agent-doc/watch.pid`)
- `--status` reports whether the daemon is running
- `--debounce` sets the debounce delay in milliseconds (default 500)

### 7.24 history

`agent-doc history <FILE>` — list exchange versions from git history.

1. Scan git log for commits touching `<FILE>`
2. Extract the `<!-- agent:exchange -->` component content at each commit
3. Display a list of commits with timestamps and content previews

`agent-doc history <FILE> --restore <COMMIT>` — restore a previous exchange version.

1. Read the exchange content from the specified commit
2. Prepend the old exchange content into the current document's exchange component
3. The restored content appears above the current exchange, preserving both

### 7.25 terminal

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

### 7.26 preflight

`agent-doc preflight <FILE>` — run all pre-agent steps and output JSON.

Combines recover, commit, claims-log check, diff, and document HEAD read into a single call. The SKILL workflow consumes the structured JSON output instead of making separate CLI calls.

**Steps (in order):**
1. Recover orphaned pending responses (`agent-doc recover`)
2. Commit previous cycle (`agent-doc commit`)
3. Read and truncate `.agent-doc/claims.log`
3c. Check linked docs: inspect `links` from frontmatter — local files compared by git commit time, URLs fetched via `ureq` with HTML-to-markdown conversion (htmd), cached in `.agent-doc/links_cache/`
4. Compute diff between snapshot and current document
5. Read document HEAD from disk

**Output (JSON to stdout):**
```json
{
  "recovered": false,
  "committed": true,
  "claims": [],
  "diff": "unified diff text or null",
  "no_changes": false,
  "document": "full document content",
  "linked_changes": [{"path": "https://example.com", "summary": "content changed (1234 bytes)", "exists": true}]
}
```

- `no_changes` is `true` when the diff is `None` (snapshot == document)
- `diff` is `null` when `no_changes` is `true`
- `document` always contains the current HEAD content
- `linked_changes` lists changes in linked docs/URLs since last cycle (omitted when empty)
- Progress/diagnostic messages go to stderr

**URL link processing:**
- URLs (`http://`/`https://`) in `links` frontmatter are fetched with a 10s timeout
- HTML responses are converted to markdown via `htmd` (stripping script, style, nav, footer, noscript, svg)
- Content is cached at `.agent-doc/links_cache/<sha256(url)>.txt`
- Changes detected by comparing fresh fetch against cached content

## 7.27 Preflight Mtime Debounce

The `preflight` command applies a 500ms mtime debounce gate: if the document's filesystem mtime is less than 500ms old, preflight waits until the file has been idle for at least 500ms. This prevents duplicate preflight runs caused by rapid sequential file saves from the editor.

## 7.28 Unified Diff Context Radius

Diff output now uses a 5-line context radius (unified diff with 5 lines of surrounding context around each hunk). This gives the agent better surrounding context to understand changes.

## 7.29 Route --debounce

`agent-doc route <FILE> [--debounce MS]` — optional debounce flag to coalesce rapid editor triggers. When set, route will skip execution if another route call for the same file completed within the debounce window.

## 7.30 is_tracked FFI Export

`agent_doc_is_tracked(path)` — C ABI export for editor plugins. Returns whether the given file path is tracked in `sessions.json` (has a registered session). Plugins use this via JNA/FFI to conditionally show UI elements for tracked documents.

## 7.31 Sync auto_start_no_wait

The sync path uses `auto_start_no_wait` instead of the standard auto-start. This variant accepts `col_args: &[String]` and computes `split_before` via `is_first_column(file, col_args)`, so new panes split in the correct direction for their column position (left-column files split before, right-column files split after). It does not block waiting for the `❯` prompt to appear (unlike `route` which waits up to 30s), avoiding sync blocking on slow Claude startup when arranging multiple panes. The call site in `sync.rs` passes the `col_args` slice through from the CLI arguments.

## 7.32 Sync Swap-Pane Atomic Reconcile

The sync path uses swap-pane atomic transitions via tmux-router. When reconciling pane layout, `auto_start_no_wait` spawns sessions without blocking on prompt detection. A `context_session` parameter allows cross-session override — sync knows which session it's managing and passes that context to `auto_start`, which takes priority over the document's `tmux_session` frontmatter field.

## 7.33 Sync tmux_session Auto-Repair (Deprecated Field)

> **Note:** `tmux_session` in frontmatter is deprecated. This auto-repair mechanism exists for backward compatibility during the deprecation period and will be removed when the field is removed.

When `context_session` (from `sync --window`) differs from the document's `tmux_session` frontmatter value, both `auto_start` and the sync loop automatically repair the frontmatter via direct string replacement. This avoids frontmatter round-trip issues (extra newlines) and ensures the document reflects the actual session assignment after cross-session moves.

## 7.34 Sync Resync Report-Only

The post-sync `resync` call runs with `--fix` disabled (report only). `auto_start` with `context_session` intentionally places panes in a different session than the frontmatter originally specified — `resync --fix` would incorrectly kill these cross-session panes. The resync still reports anomalies for operator awareness.

## 7.35 Sync Visible-Window Split

When the sync path (`skip_wait=true`) creates new panes, it prefers splitting in the visible `agent-doc` window of the target session rather than falling back to any registered pane (which may be in a stash window). This ensures new panes appear where the user can see them. Falls back to `find_registered_pane_in_session` if no panes exist in the agent-doc window.

## 7.36 Repair Layout

`repair_layout` normalizes the tmux window layout before every sync. It receives the tmux handle, session name, and target window name (always `"agent-doc"`). The plugin always passes `--window agent-doc` as a fallback so the target window name is known.

**Phase 1 — Stash consolidation:** Merges all secondary stash windows (`stash-*` and duplicate `stash` windows) into a single primary stash window. For each secondary, all panes are joined into the primary via `join-pane -dv`, targeting the largest pane to avoid "pane too small" errors. Empty secondary windows are killed after pane migration.

**Phase 2 — Window rescue:** If the target `agent-doc` window does not exist, attempts to recreate it by finding an alive registered pane in the stash (via `sessions::load()`), breaking it out with `break-pane`, and renaming the resulting window to `agent-doc`.

**Phase 3 — Index normalization:** Re-lists windows after Phases 1+2 and moves the `agent-doc` window to index 0 via `move-window` if it is not already there. This phase always runs.

**Fast path:** When the target window already exists and there is at most one stash window, Phases 1 and 2 are skipped entirely. Only Phase 3 (index normalization) executes, making the common case a lightweight check.

### 7.27 session

`agent-doc session` — show the configured tmux session.
`agent-doc session set <name>` — update config.toml and migrate panes to the new session.

**Show:** Reads `.agent-doc/config.toml` `tmux_session` field and prints it (or "(none)").

**Set:** Updates config.toml, then moves the `agent-doc` window and `stash` window from the old session to the new one via `tmux move-window`. If the move fails (target session doesn't exist), config is still updated — subsequent route/claim operations will target the new session.

**Session resolution (`resolve_target_session`):** Single function in route.rs that all session-targeting code paths use. Priority: (1) context_session from sync --window, (2) config.toml if alive, (3) fallback to current session. Config is auto-updated only when the configured session is dead.

### 7.28 dedupe

`agent-doc dedupe <FILE>` — remove consecutive duplicate response blocks.

Detects consecutive `### Re:` blocks with identical content (after stripping boundary markers) and removes the duplicate. Updates the snapshot after removal. Idempotent — running twice produces the same result.

## 8. Session Routing

### 8.1 Registry

`sessions.json` maps document session UUIDs to tmux panes:

```json
{
  "cf853a21-...": {
    "pane": "%4",
    "pid": 12345,
    "cwd": "/path/to/project",
    "started": "2026-02-25T21:24:46Z",
    "file": "tasks/plan.md",
    "window": "1"
  }
}
```

Multiple documents can map to the same pane (one Claude session, multiple files). The `window` field (optional) enables window-scoped routing — `claim --window` and `layout --window` use it to filter panes to the correct IDE window.

### 8.2 Use Cases

| # | Scenario | Command | What Happens |
|---|---|---|---|
| U1 | First session for a document | `agent-doc start plan.md` | Creates tmux pane, launches Claude, registers pane |
| U2 | Submit from JetBrains plugin | Plugin `Ctrl+Shift+Alt+A` | Calls `agent-doc route <file>` → sends to registered pane |
| U3 | Submit from Claude Code | `/agent-doc plan.md` | Skill invocation — diff, respond, write back |
| U4 | Claim file for current session | `/agent-doc claim plan.md` | Skill delegates to `agent-doc claim` → updates sessions.json |
| U5 | Claim after manual Claude start | `/agent-doc claim plan.md` | Fixes stale pane mapping without restarting |
| U6 | Claim multiple files | `/agent-doc claim a.md` then `/agent-doc claim b.md` | Both files route to same pane |
| U7 | Re-claim after reboot | `/agent-doc claim plan.md` | Overrides old pane mapping (last-call-wins) |
| U8 | Pane dies, plugin submits | Plugin `Ctrl+Shift+Alt+A` | `route` detects dead pane → auto-start cascade |
| U9 | Install skill in new project | `agent-doc skill install` | Writes bundled SKILL.md to `.claude/skills/agent-doc/` |
| U10 | Check skill version after upgrade | `agent-doc skill check` | Reports "up to date" or "outdated" |
| U11 | Permission prompt from plugin | PromptPoller polls `prompt --all` | Shows bottom bar with numbered hotkeys in IDE |
| U12 | Claim notification in session | Skill reads `.agent-doc/claims.log` | Prints claim records, truncates log |
| U13 | Clean up dead pane mappings | `agent-doc resync` | Removes stale entries from sessions.json |

### 8.3 Claim Semantics

`claim` binds a document to a **tmux pane**, not a Claude session. The pane is the routing target — `route` sends keystrokes to the pane. Claude sessions come and go (restart, resume), but the pane persists. If Claude restarts on the same pane, routing still works without re-claiming.

Last-call-wins: any `claim` overwrites the previous mapping for that document's session UUID.

### 8.4 Stash Window Routing

The stash system preserves running Claude sessions when the user switches editor tabs. Panes are moved to a hidden stash window rather than killed, keeping the Claude session alive for later reuse.

**Window-scoped routing:** Each editor split maps to a tmux pane in the primary window (`@0`). When the user switches files, `reconcile()` swaps panes by detaching unwanted ones into the stash and attaching needed ones back.

**Stash lifecycle:**

| Phase | Operation | Detail |
|-------|-----------|--------|
| DETACH | `stash_pane()` | Moves an unwanted pane into the stash window via `tmux join-pane` |
| — | target selection | Targets the LARGEST pane in the stash (by height) to avoid "pane too small" errors |
| — | overflow | If join fails, `break_pane_to_stash()` creates an overflow stash window (also named `"stash"`) |
| ATTACH | `reconcile()` | Joins a stashed pane back into `@0` when needed again |
| RESCUE | `sync` pre-resolution | Rescues stashed panes back to agent-doc window via `swap-pane`/`join-pane` before layout |

**Discovery:** `find_all_stash_windows()` returns all stash windows — both the primary stash and any overflow windows. All windows named `"stash"` or matching `"stash-*"` (tmux auto-deduplication) are treated as stash windows by `is_stash_window_name()`.

**Invariants:**
- Stashed panes keep running — the Claude session remains alive inside
- Stash windows are named `"stash"` for consistent discovery
- The stash window is resized to 200 rows before join operations to prevent minimum-size failures
- Focus never leaves window `@0` during stash operations (`-d` flags are always set)

**Commit write contract:** `commit()` only modifies the snapshot (appending HEAD markers and repositioning the boundary to end-of-exchange). The working tree file is NEVER written by `commit()`. All visible document changes are delivered via IPC through the plugin Document API. This prevents IDE file-cache conflicts and keystroke loss that would occur if `commit()` wrote to disk while the user is typing.

**Snapshot boundary cleanup:** After committing, `commit()` calls `reposition_boundary_to_end()` on the snapshot content. This uses `remove_all_boundaries()` to strip ALL stale boundaries from the snapshot (not just the last one), then inserts a single fresh 8-char boundary at end-of-exchange. The cleaned snapshot is saved back. This guarantees the snapshot never accumulates stale boundaries regardless of plugin behavior.

**Boundary reposition lifecycle:**
1. **Before IPC patch JSON (`reposition_boundary_to_end()`):** All IPC write paths (`run_ipc`, `try_ipc`, IPC-timeout fallback) read the on-disk document and call `reposition_boundary_to_end()` in memory. This removes ALL stale boundaries and inserts a single fresh one. The repositioned document is used solely to extract `boundary_id` values — never written to disk. This ensures the `boundary_id` points to end-of-exchange (after the user's prompt), not the stale mid-exchange position.
2. During `agent-doc write`: the `reposition_boundary: true` IPC flag tells the plugin to move the boundary after applying the response patch. The plugin should call `agent_doc_reposition_boundary_to_end()` via FFI to ensure identical cleanup logic.
3. During `agent-doc commit`: (a) the snapshot is cleaned via `reposition_boundary_to_end()`, and (b) a standalone IPC signal (`try_ipc_reposition_boundary`) sends a lightweight reposition-only patch (no content changes, 500ms timeout). This ensures the boundary is at end-of-exchange immediately after commit, so user text typed before the next write cycle is positioned correctly.
4. If no plugin is active, both IPC signals are silently skipped — the snapshot still has the correct boundary position

## 9. Git Integration

- Commit: `git add -f {file}` (bypasses .gitignore) + `git commit -m "agent-doc: {timestamp}" --no-verify`
- Branch: `git checkout -b agent-doc/{filestem}`
- Squash: soft-reset to before first `agent-doc:` commit, recommit as one

## 9.5 Hook System

Cross-session event coordination via `agent-kit` hooks (v0.3).

**CLI:** `agent-doc hook fire|poll|listen|gc`

- `fire <EVENT> <FILE>` — write event JSON to `.agent-doc/hooks/<event>/`, auto-reads session ID from frontmatter
- `poll <EVENT> [--since SECS]` — read events newer than timestamp, clean expired
- `listen [--root PATH]` — start Unix socket listener at `.agent-doc/hooks.sock`
- `gc [--root PATH]` — clean expired events across all hooks

**Lifecycle hooks fired by agent-doc:**
- `post_write` — after IPC write succeeds (from `write.rs`)
- `post_commit` — after successful git commit (from `git.rs`)
- `claim` / `layout_change` — available via CLI, not yet wired into binary paths

**Transport:** `HookTransport` trait with `FileTransport` (default), `SocketTransport` (Unix socket), `ChainTransport` (fallback chain). Socket transport connects to `.agent-doc/hooks.sock` and expects `ok\n` ack.

**Claude Code bridge:** Add `PostToolUse` hook to `settings.json`:
```json
{"hooks":{"PostToolUse":[{"matcher":"Write|Edit","command":"agent-doc hook fire post_write \"$TOOL_INPUT_FILE\""}]}}
```

## 10. Security

agent-doc is designed for single-user, local operation. There is no authentication, authorization, or multi-user access control.

### 10.1 Threat Model

- **Trusted user, untrusted content.** The user is trusted; document content may contain prompt injection from external sources (pasted emails, web pages, chat logs).
- **Local filesystem scope.** All data (documents, snapshots, exchange history, links cache) resides on the local filesystem. No network services are exposed.
- **Git as audit trail.** All agent responses are committed to git, providing a complete audit trail. However, git history may contain sensitive content if documents reference private data.

### 10.2 Known Risks

- **Prompt injection via document content.** Content pasted from external sources could contain injection attempts. The agent processes all document content as user input with no injection scanning. Mitigation: user awareness; planned content scanning in `agent-doc write`.
- **`--dangerously-skip-permissions` exposure.** When running with this flag (common in agent-doc sessions via `claude_args` frontmatter), the agent has full filesystem access. Injected prompts could read files or execute commands.
- **Data divulgence through the response channel.** Even with sandboxing, the agent's response IS the output channel. If the model has sensitive data in context, injection can convince it to include that data in the document response. The only real defense is context isolation (see ragie-web-doc security analysis).
- **Links cache may contain sensitive fetched content.** URL content fetched via `links` frontmatter is cached at `.agent-doc/links_cache/`. This cache is not encrypted and persists until manually cleared.

### 10.3 Recommendations

- Use a **private git repository** for the project containing session documents.
- Avoid putting secrets (API keys, credentials) in documents or agent context.
- For shared/collaborative use cases, wait for the planned multi-user security model (access control, session isolation, content scanning).
- Review agent responses before sharing or publishing document content.

## 11. Debounce System Gaps and Limitations

The debounce subsystem manages multi-layer typing detection across editor plugins (JetBrains, VS Code, Neovim, Zed) and CLI invocations. While the architecture is sound, several known gaps exist that should inform operators and guide future improvements.

### 11.1 Mtime Granularity in Route Path

**Gap:** The route path relies on filesystem mtime for debouncing rapid edits. Filesystem mtime resolution varies:
- **Coarse-grained systems** (e.g., HFS+ on macOS): 1-second resolution
- **Fine-grained systems** (Linux ext4): ~100ms resolution

When multiple edits occur within the mtime granularity window, route may miss the intermediate change and only detect the final state.

**Impact:** Rare but real on macOS. User typing very fast may trigger a route call with an editor state that reflects only partial changes.

**Mitigation:** Route path uses a timeout cap (10× debounce duration) to prevent indefinite hangs. Cross-process typing indicator files provide additional fallback for preflight detection.

**Test coverage:** `rapid_edits_within_mtime_granularity` documents this edge case. See `src/debounce.rs`.

### 11.2 Untracked File Edge Case

**Gap:** Files passed to `document_changed()` are tracked in the in-process `LAST_CHANGE` map. Files never passed to `document_changed()` return `idle=true` immediately (design choice to prevent `await_idle` blocking forever on unknown files).

This means the CLI cannot distinguish:
- "File was tracked and has been idle for 2s"
- "File was never tracked by any plugin"

**Impact:** Low. The `is_tracked()` function exists to distinguish these cases, but callers must explicitly check. Non-blocking probes may conservatively assume untracked files are NOT idle.

**Mitigation:** Use `is_tracked(file)` before making assumptions about untracked files. Preflight applies both mtime debounce AND typing indicator debounce (redundant but safe).

**Test coverage:** `is_tracked_distinguishes_untracked_from_idle`, `await_idle_on_untracked_file_returns_immediately`. See `src/debounce.rs`.

### 11.3 Hash Collision in Typing Indicator Paths

**Gap:** Typing indicator files are stored in `.agent-doc/typing/<hash>` where hash is computed via `std::collections::hash_map::DefaultHasher`. DefaultHasher is non-cryptographic and designed for hash maps, not for unique identifiers.

Collision probability: ~1 in 4.3 billion for random inputs. Collision is possible but extremely unlikely.

**Impact:** Very low probability. If collision occurs, the most recent change wins (last write to the shared file). The collision is self-correcting in the next debounce cycle because file timestamps diverge.

**Mitigation:** No action needed. Collisions are rare and self-healing. If deterministic behavior is required, consider switching to SHA256 hashing in future.

**Test coverage:** `hash_collision_handling` documents detection logic. See `src/debounce.rs`.

### 11.4 Reactive Mode Assumes CRDT Merge Convergence

**Gap:** Watch daemon's reactive path (used for `agent_doc_write: crdt` documents) applies zero debounce, expecting instant re-submit on file change. This assumes the CRDT merge algorithm always converges to a consistent state.

If a CRDT merge produces unexpected results (e.g., text duplication, loss of edits), reactive mode could cause the watch daemon to re-submit with corrupted state repeatedly.

**Impact:** Medium (data loss risk if CRDT merge is broken). Mitigated by extensive CRDT testing in `src/crdt.rs` and `src/merge.rs`.

**Mitigation:** CRDT implementation is battle-tested with golden-answer test cases (20-30 cases per session diff). See `agent-doc eval-runner` for continuous validation.

**Test coverage:** `reactive_mode_requires_zero_debounce` documents the assumption. CRDT correctness is verified via `agent-doc test` suite.

### 11.5 Status File Staleness Timeout (30s Hardcoded)

**Gap:** Response status files (`.agent-doc/status/<hash>`) expire after 30 seconds with the assumption: "if no update after 30s, the operation probably crashed."

This timeout is hardcoded in `get_status_via_file()` and not configurable.

**Impact:** Medium. Long-running operations (slow CI, expensive LLM calls, network latency) may exceed 30s and be treated as crashed, allowing duplicate submissions.

**Mitigation:** For long-running scenarios, increase the timeout (currently not exposed via config). The binary also sends `set_status()` updates, so well-instrumented operations will keep the timeout alive.

**Test coverage:** `status_file_staleness_timeout`, `status_file_cleared_on_idle` document the behavior. See `src/debounce.rs`.

### 11.6 Hardcoded Timing Constants in Preflight

**Gap:** Preflight applies a hardcoded **1500ms** debounce window via `is_typing_via_file(&file_str, 1500)` in `preflight.rs:366`.

Meanwhile, the poll-based debounce used elsewhere defaults to **500ms**. This creates asymmetry:
- Typing indicator requires 1500ms to expire
- Poll-based debounce (watch, route) uses 500ms

Not configurable per-document; one-size-fits-all fails for slow CI systems or fast typists.

**Impact:** Low to Medium. CI systems that take >1500ms to write files will appear to be typing longer than expected, potentially delaying preflight. Conversely, fast typists may experience premature debounce expiry on poll-based paths.

**Mitigation:** Make timing constants configurable via frontmatter (`agent_doc_debounce_ms`, `agent_doc_typing_indicator_ms`). For now, operators can adjust via direct code modification if needed.

**Test coverage:** `timing_constants_are_configurable`, `await_idle_via_file_respects_poll_interval` document current behavior. See `src/debounce.rs`.

### 11.7 Recommended Improvements

1. **Expose timing constants to frontmatter** — Allow per-document control via:
   ```yaml
   agent_doc_debounce_ms: 500
   agent_doc_typing_indicator_ms: 1500
   agent_doc_status_timeout_ms: 30000
   ```

2. **Switch to cryptographic hashing** (SHA256) for typing indicator and status file paths to eliminate collision risk entirely.

3. **Make 30s status timeout configurable** — either via config.toml or frontmatter.

4. **Mtime fallback in route path** — If mtime-detected change is stale (>1s), also check cross-process typing indicator as fallback.

5. **CRDT merge monitoring** — Log merge conflicts and convergence issues to `.agent-doc/logs/merge.log` for operator visibility.
