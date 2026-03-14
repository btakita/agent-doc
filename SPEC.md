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
- `agent_doc_format`: Document format — `append` or `template` (default: `template`).
- `agent_doc_write`: Write strategy — `merge` or `crdt` (default: `crdt`).
- `agent_doc_mode`: **Deprecated.** Single field mapping: `append` → format=append, `template` → format=template, `stream` → format=template+write=crdt. Explicit `agent_doc_format`/`agent_doc_write` take precedence. Legacy aliases: `mode`, `response_mode`.
- `agent`: Agent backend name (overrides config default)
- `model`: Model override (passed to agent backend)
- `branch`: Reserved for branch tracking

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

Per-component behavior is configured in `.agent-doc/components.toml` (see §7.20).

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

Fields: `default_agent`, `[agents.{name}]` with `command`, `args`, `result_path` (reserved), `session_path` (reserved).

## 7. Commands

### 7.1 run

`agent-doc run <FILE> [-b] [--agent NAME] [--model MODEL] [--dry-run] [--no-git]`

1. Compute diff → 2. Build prompt (diff + full doc) → 3. Branch if `-b` → 4. Send to agent → 5. Update session ID → 6. Append response → 7. Save snapshot → 8. `git add -f` + commit

First run prompt wraps full doc in `<document>` tags. Subsequent wraps diff in `<diff>` tags + full doc in `<document>`.

### 7.2 init

`agent-doc init <FILE> [TITLE] [--agent NAME]` — scaffolds frontmatter + `## User` block. Fails if exists.

### 7.3 diff

`agent-doc diff <FILE>` — prints unified diff to stdout.

### 7.4 reset

`agent-doc reset <FILE>` — clears session ID, deletes snapshot.

### 7.5 clean

`agent-doc clean <FILE>` — squashes all `agent-doc:` commits for file into one via `git reset --soft`.

### 7.6 audit-docs

`agent-doc audit-docs [--root DIR]` — checks CLAUDE.md/AGENTS.md/README.md/SKILL.md for tree path accuracy, line budget (1000), staleness, and actionable content. Exit 1 on issues.

`--root DIR` overrides auto-detection of the project root directory. Without it, the root is resolved via project markers (Cargo.toml, package.json, etc.), then `.git`, then CWD fallback.

### 7.7 start

`agent-doc start <FILE>` — start Claude in a new tmux pane and register the session.

1. Ensure session UUID in frontmatter (generate if missing)
2. Read `$TMUX_PANE` (must be inside tmux)
3. Register session → pane in `sessions.json`
4. Exec `claude` (replaces process)

### 7.8 route

`agent-doc route <FILE> [--pane P]` — route a `/agent-doc` command to the correct tmux pane.

1. Prune stale entries from `sessions.json`
2. Ensure session UUID in frontmatter (generate if missing)
3. Look up pane in `sessions.json`
4. If pane alive → `tmux send-keys` `/agent-doc <FILE>` + Enter, focus pane
5. If pane dead or unregistered → lazy-claim to active pane in `claude` tmux session (or `--pane P`), register, send command, auto-sync layout for all files in the same window
6. If no active pane available → auto-start cascade (create session/window, start Claude, register)

### 7.9 claim

`agent-doc claim <FILE> [--position left|right|top|bottom] [--window W] [--pane P]` — claim a document for a tmux pane.

1. Ensure session UUID in frontmatter (generate if missing)
2. **Resolve effective window** (see Window Resolution below)
3. Determine pane: `--pane P` overrides, else `--position` resolves via tmux pane geometry, else `$TMUX_PANE`
4. Register session → pane in `sessions.json`, including window ID

Unlike `start`, does not launch Claude — the caller is already inside a Claude session. Last-call-wins: a subsequent `claim` for the same file overrides the previous pane mapping. `--position` is used by the JetBrains plugin to map editor split positions to tmux panes.

**Window Resolution:**

When `--window W` is provided:

1. Check if window `W` is alive (`tmux list-panes -t W`)
2. If alive → use `W` (no change)
3. If dead → scan `sessions.json` for entries with matching project `cwd` and non-empty `window` field. For each, check liveness. Use first alive match.
4. If no alive windows found → fall through to no-window behavior (position detection without window scoping)

This prevents the JetBrains plugin from hitting persistent error balloons when a tmux window dies. The same fallback pattern is used in `sync.rs` for dead `--window` handling.

**Notifications:**
- `tmux display-message` — 3-second overlay on the target pane showing "Claimed {file} (pane {id})"
- `.agent-doc/claims.log` — appends `Claimed {file} for pane {id}` for deferred display by the SKILL.md workflow on next invocation

### 7.10 focus

`agent-doc focus <FILE> [--pane P]` — focus the tmux pane for a session document.

1. Read session UUID from file's YAML frontmatter (or use `--pane` override)
2. Look up pane ID in `sessions.json`
3. Run `tmux select-window -t <pane-id>` then `tmux select-pane -t <pane-id>`

Exits with error if the pane is dead or no session is registered.

### 7.11 layout

`agent-doc layout <FILE>... [--split h|v] [--window W]` — arrange tmux panes to mirror editor split layout.

1. Resolve each file to its session pane via frontmatter → `sessions.json`
2. If `--window` given, filter to panes registered for that window only
3. Pick the target window (the one containing the most wanted panes; tiebreak: most total panes)
4. Break out only registered session panes that aren't wanted (shells and tool panes are left untouched)
5. Join remaining wanted panes into the target window (`tmux join-pane`)
6. Focus the first file's pane (the most recently selected file)

`--split h` (default): horizontal/side-by-side. `--split v`: vertical/stacked. Single file falls back to `focus`. Dead panes and files without sessions are skipped with warnings.

### 7.12 resync

`agent-doc resync` — validate sessions.json against live tmux panes.

1. Load `sessions.json`
2. For each entry, check if the pane is alive via `tmux has-session`
3. Remove entries with dead panes
4. Report removed entries and remaining active sessions

### 7.13 prompt

`agent-doc prompt <FILE>` — detect permission prompts from a Claude Code session.

- Captures tmux pane content, strips ANSI, searches for numbered-option patterns
- Returns JSON: `{ "active": bool, "question": str, "options": [...] }`
- `--answer N` navigates to option N and confirms
- `--all` polls all live sessions, returns JSON array

### 7.14 commit

`agent-doc commit <FILE>` — git add + commit with auto-generated timestamp.

### 7.15 skill

`agent-doc skill install` — write the bundled SKILL.md to `.claude/skills/agent-doc/SKILL.md` in the current project. Idempotent (skips if content matches).

`agent-doc skill check` — compare installed skill vs bundled version. Exit 0 if up to date, exit 1 if outdated or missing.

The bundled SKILL.md contains an `agent-doc-version` frontmatter field set to the binary's version at build time. When the skill is invoked via Claude Code, the pre-flight step compares this field against the installed binary version (`agent-doc --version`). If the binary is newer, `agent-doc skill install` runs automatically to update the skill before proceeding.

### 7.16 outline

`agent-doc outline <FILE> [--json]` — display markdown section structure with line counts and approximate token counts.

1. Read file, skip YAML frontmatter
2. Parse `#`-prefixed headings into a section tree
3. For each section: heading text, depth, line number, content lines, approximate tokens (bytes/4)
4. Content before the first heading appears as `(preamble)`

Default output: indented text table. `--json` outputs a JSON array of section objects (`heading`, `depth`, `line`, `lines`, `tokens`).

### 7.17 upgrade

`agent-doc upgrade` — check crates.io for latest version, upgrade via GitHub Releases binary download → cargo install → pip install (cascade).

> **Startup version check:** On every invocation (except `upgrade` itself), `warn_if_outdated` queries crates.io (with a 24h cache at `~/.cache/agent-doc/version-cache.json`) and prints a one-line stderr warning if a newer version is available. Errors are silently ignored so normal operation is never blocked.

### 7.18 plugin

`agent-doc plugin install <EDITOR>` — download and install the editor plugin from the latest GitHub Release.

`agent-doc plugin update <EDITOR>` — update an installed plugin to the latest version.

`agent-doc plugin list` — list available editor plugins and their install status.

Supported editors: `jetbrains`, `vscode`. Downloads plugin assets from GitHub Releases (`btakita/agent-doc`). Prefers signed assets (`*-signed.zip`) when available, falling back to unsigned. Auto-detects standard plugin directories for each editor (e.g., JetBrains plugin dir via `idea.plugins.path` or platform defaults, VS Code `~/.vscode/extensions/`).

### 7.19 sync

`agent-doc sync --col <FILES>,... [--col <FILES>,...] [--window W] [--focus FILE]` — declarative 2D layout sync.

Mirrors a columnar editor layout in tmux. Each `--col` is a comma-separated list of files. Columns arrange left-to-right; files stack top-to-bottom within each column.

**Reconciliation algorithm** (simple 2-step detach/attach):
1. **SNAPSHOT** — query current pane order in target window
2. **FAST PATH** — if current order matches desired, done
3. **DETACH** — `break-pane` unwanted panes out of target window (panes stay alive in solo windows)
4. **ATTACH** — `join-pane` missing desired panes into target window (isolate from shared windows first, then join with correct split direction: `-h` for columns, `-v` for stacking)
5. **REORDER** — if all panes present but wrong order, break non-first panes out and rejoin in order
6. **VERIFY** — confirm final layout matches desired order

### 7.20 patch

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
pre_patch = "cmd"      # Shell command: stdin→stdout transform
post_patch = "cmd"     # Shell command: fire-and-forget
```

### 7.21 write

`agent-doc write <FILE> [--baseline-file PATH] [--stream] [--ipc]` — apply patch blocks from stdin to a template document.

1. Read response (patch blocks) from stdin
2. Parse `<!-- patch:name -->...<!-- /patch:name -->` blocks
3. Read document and baseline (from `--baseline-file` or current file)
4. Apply patches to baseline:
   - **Exchange component uses replace mode** (overrides the default append), since the `--stream` path receives the complete intended exchange content. Without this override, the user's prompt (already in the baseline exchange) would be duplicated by the append.
   - All other components use their configured mode (from `.agent-doc/components.toml`) or default (`replace`)
5. CRDT merge: if the file was modified during response generation, merge `content_ours` (baseline + patches) with `content_current` (file on disk) using Yrs CRDT
6. Atomic write + snapshot save + CRDT state save

**`--stream` flag:** Enables CRDT write strategy. Required for template/CRDT documents.

**`--ipc` flag:** Writes a JSON patch file to `.agent-doc/patches/` for IDE plugin consumption instead of modifying the document directly.

### 7.22 watch

`agent-doc watch [--stop] [--status] [--debounce MS] [--max-cycles N]` — watch session files for changes and auto-submit.

- Watches files registered in `sessions.json` for modifications (via `notify` crate)
- On file change (after debounce), runs `submit::run()` on the changed file
- **Reactive mode:** CRDT-mode documents (`agent_doc_write: crdt`) are discovered with `reactive: true` and use zero debounce (`Duration::ZERO`) for instant re-submit on file change. Reactive paths are tracked in a `HashSet<PathBuf>`.
- **Loop prevention:** changes within the debounce window after a submit are treated as agent-triggered; agent-triggered changes increment a cycle counter; if content hash matches previous submit, stop (convergence); hard cap at `--max-cycles` (default 3)
- `--stop` sends SIGTERM to the running daemon (via `.agent-doc/watch.pid`)
- `--status` reports whether the daemon is running
- `--debounce` sets the debounce delay in milliseconds (default 500)

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

## 9. Git Integration

- Commit: `git add -f {file}` (bypasses .gitignore) + `git commit -m "agent-doc: {timestamp}" --no-verify`
- Branch: `git checkout -b agent-doc/{filestem}`
- Squash: soft-reset to before first `agent-doc:` commit, recommit as one
