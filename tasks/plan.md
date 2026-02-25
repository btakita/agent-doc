---
session: cf853a21-3aaf-4775-9cc8-0e789109f9dd
agent: null
model: null
branch: null
---

# Plan: agent-doc & agent-loop

## Status

- **agent-doc**: v0.3.0 — crates.io + GitHub Releases. 87 tests passing (66 unit + 21 integration).
  - Tmux session routing: `start`, `route`, `prompt` subcommands with auto-start cascade
  - `Tmux` struct with `-L` socket isolation for testing (9 tmux isolation tests)
  - Session registry (`sessions.json`) — UUID → tmux pane mapping, liveness checks, auto-start, file path
  - Prompt detection: `agent-doc prompt <file>` captures pane, parses Claude Code prompts → JSON
  - Multi-session polling: `agent-doc prompt --all` polls all live sessions, returns JSON array
  - Prompt answering: `agent-doc prompt --answer <N> <file>` navigates + confirms via arrow keys
  - `agent-doc commit <file>` — git add + commit with generated timestamp
  - `send_keys` split: literal text (`-l`) + 50ms delay + separate Enter (Claude Code TUI fix)
  - `new_window -a`: append flag for next available window index
  - `AGENT_DOC_NO_AUTOSTART`: env var to prevent auto-start in tests
  - Upgrade check + self-update via GitHub Releases
  - Auto-frontmatter: `ensure_session()` generates UUID v4 for documents without one
  - Separated `session` (routing UUID) from `resume` (Claude conversation ID) in frontmatter
- **corky**: v0.10.0 — crates.io + GitHub Releases. 290 tests passing.
  - Social media posting: LinkedIn OAuth2, draft creation/publishing, 6 CLI subcommands, 54 social-specific tests
  - Contact sync v2: conversation-aware eligibility, explicit sharing, alias matching, 3-way merge
  - Topics: config, list, add, info, suggest (8 topic tests)
  - Scheduling: `corky schedule run/list` (concurrent work by other agent)
  - Upgrade check + self-update via GitHub Releases
- **instruction-files**: v0.1.0 — crates.io. 45 tests passing.
  - Shared audit crate: staleness, tree paths, line budget, actionable content checks
  - `AuditConfig::agent_doc()` / `AuditConfig::corky()` presets (planned: collapse to single polyglot default)
  - README with usage, config reference, consumer links
- **agent-doc-jetbrains**: v0.1.0 — signed plugin.
  - Hotkey (`Ctrl+Shift+Alt+A`) + right-click context menu
  - `SubmitAction`: saves all documents before routing, starts prompt poller after submit
  - `TerminalUtil`: `resolveAgentDoc()` checks `~/bin`, `~/.local/bin`, `~/.cargo/bin`, `/usr/local/bin`
  - `PromptPoller`: polls `agent-doc prompt` every 1.5s after submit, syncs VirtualFile via `modificationStamp` (one-shot reload on change)
  - `PromptPanel`: bottom-anchored bar with numbered hotkeys (1-9, Esc to dismiss), font scales with IDE editor font
  - Inline cursor hint on success (`HintManager`), balloon notification on error
  - `signPlugin` depends on `buildPlugin`, reads from `certificate/` files
- **agent-doc-vscode**: v0.1.0 — extension. Same hotkey, calls `agent-doc route` via `child_process.exec`.
- **agent-loop**: Root Makefile with `make release`, `make update`, `make check`, `make precommit`, `make publish`, `make clean`.

### Session Routing (tmux)

```
agent-doc start <file>    # register pane in sessions.json, exec claude
agent-doc route <file>    # look up session from frontmatter, send-keys to pane, auto-start if needed
agent-doc prompt <file>   # capture pane, detect permission prompts → JSON
agent-doc prompt --answer <N> <file>  # answer a prompt by selecting option N
```

**Frontmatter:**
```yaml
---
session: cf853a21-...    # permanent routing UUID (for tmux pane lookup)
resume: abc123...        # Claude conversation ID (for --resume)
---
```

**Auto-start cascade** (when pane is dead/missing):
1. tmux not running → `tmux new-session -s claude`
2. "claude" session missing → create it
3. "claude" session exists → `tmux new-window -a`
4. Start `agent-doc start` in new pane

## Outstanding

### agent-doc

- [ ] `agent-doc register <file>` — register current tmux pane without starting a new session
- [x] `agent-doc prompt --all` — poll all active sessions (multi-session support)
- [x] `agent-doc commit <file>` — git add + commit with generated timestamp
- [ ] Codex backend (`src/agent/codex.rs`) — plan at `tasks/integrations/codex.md`

### agent-doc-jetbrains

- [x] Multi-session prompt polling — track all submitted files, poll via `--all`
- [x] PromptPanel file context — prepend `[filename]` to prompt question

### instruction-files

- [ ] Collapse `AuditConfig` presets to single polyglot `Default` (always audit both AGENTS.md + CLAUDE.md)
- [ ] `.instruction-files.toml` project-root config file (override extensions, root markers, skip dirs)

### corky — Topics

- [x] `TopicConfig` struct + wire into `CorkyConfig` deserialization
- [x] `corky topics list [--verbose]` — show configured topics
- [x] `corky topics add <name> --keyword K` — add topic to `.corky.toml`
- [x] `corky topics info <name>` — show details + matching conversations
- [x] `corky topics suggest [--limit N] [--mailbox M]` — frequency histogram
- [ ] Extract core 3-way merge from `contact/sync.rs` → shared `context_sync` module
- [ ] Topic sync: eligibility check (label/contact match OR `shared_with`), delegate to shared merge
- [ ] Thread tagging: auto-add `**Topic**: {name}` to matching conversations during sync

### corky — Telegram

- [ ] Phase 2: Live sync via `grammers-client` — plan at `corky/tasks/integrations/telegram.md`
- [ ] Phase 3: Watch mode (real-time push)
- [ ] Phase 4: Send via `draft push`

### corky — Slack

- [ ] Phase 2: API sync via `slack-morphism` — plan at `corky/tasks/integrations/slack.md`
- [ ] Phase 3: Socket Mode (real-time push)

### Future

- [ ] Streaming responses (agent-doc)
- [ ] Document-only permission flow — no terminal (agent-doc)
- [ ] Desktop notifications on completion (agent-doc)
- [ ] Multi-agent documents — sections routed to different agents (agent-doc)
- [ ] Email transport for async human-agent loops — bridge with corky (agent-doc)
- [ ] Personal data separation — nested git repo inside `tasks/` (agent-loop)

## Design Decisions

### Frontmatter: session vs resume

`session` is the permanent document routing UUID (generated once, never changes). `resume` is the Claude conversation ID (set after first submit, cleared by `agent-doc reset`). Separating them prevents tmux routing breakage when Claude overwrites the session ID.

### instruction-files: path + version dependency

```toml
instruction-files = { path = "../instruction-files", version = "0.1" }
```

Local builds use path. `cargo publish` uses crates.io version. Publish order: instruction-files → agent-doc → corky.

### instruction-files: polyglot default + .instruction-files.toml

Collapse to a single `AuditConfig::default()` that always discovers and audits both `AGENTS.md` and `CLAUDE.md`. Convention: `CLAUDE.md -> AGENTS.md` symlink so both names resolve.

Config at `.instruction-files.toml` in project root (optional). Resolution: config file → `AuditConfig::default()`. Consuming binaries read the TOML — zero new deps in the library crate.

### tmux routing: direct, not broadcast

Each document has its own session UUID → tmux pane. Direct `send-keys -t <pane>` routing. Broadcasting (`synchronize-panes`) would send the same command to all panes. Extract as separate crate when a second consumer exists.

### Prompt detection: tmux capture-pane polling

`agent-doc prompt` captures the visible terminal via `tmux capture-pane -t <pane> -p`, strips ANSI escape codes, and searches for Claude Code's numbered-option prompt pattern (e.g., "Do you want to proceed? 1. Yes 2. No"). Returns structured JSON. The JetBrains plugin polls this every 1.5s after submit and shows a bottom-anchored bar with hotkey support.

Fragile by nature (terminal scraping), but works for common cases. Future: Claude Code hooks API or `--json-events` mode.

### Topics — `.corky.toml` Config

```toml
[topics.project-alpha]
labels = ["project-alpha", "alpha-dev"]
contacts = ["alice", "bob"]
shared_with = ["alice", "bob"]
```

### Context Routing

Topics, contacts, and threads share the same sync primitive — content-hash-based 3-way merge with eligibility rules.

## Reference

- **Decisions & architecture:** `agent-doc/SPECS.md`
- **Module layout & conventions:** `agent-doc/CLAUDE.md`
- **Workspace structure:** `SPECS.md`
- **Workflow orchestration:** `CLAUDE.md`

## Session Log

### Session 1 (2026-02-25)

1. **Pending releases** — agent-doc has 2 unreleased commits (prompt detection, send-keys fix); instruction-files needs git tag v0.1.0; corky up to date
2. **JetBrains PromptPanel font** — scaled with IDE editor font via `EditorColorsManager` (question: +1, buttons: base, hint: -2)
3. **Topics implemented** — Phases 1-3 complete (config, CLI, suggest). 8 new tests, 290 total passing. No conflicts with scheduling agent.
4. **PromptPoller fixes** — sync refresh (async→sync), one-shot reload via `modificationStamp` tracking
5. **Multi-session polling plan** — designed 4-phase plan (SessionEntry file field → `--all` CLI → plugin multi-poll → file context in PromptPanel). Not yet implemented.

## User

Implement **Multi-session polling plan**

The git tag whenever resume.md or plan.md is updated uses $() substitution to get the timestamp. Please add the functionality directly to agent-doc.

## Assistant

Both done. 87 tests passing, clippy clean, plugin built and signed.

### Multi-session prompt polling — implemented

**Phase 1: `SessionEntry.file`** — `agent-doc/src/sessions.rs`
- Added `file: String` field with `#[serde(default)]` for backward compat
- `register()` now takes a `file` parameter
- Updated `start.rs` and `route.rs` callers

**Phase 2: `agent-doc prompt --all`** — `agent-doc/src/prompt.rs` + `main.rs`
- New `PromptAllEntry` struct (flattened `session_id` + `file` + `PromptInfo`)
- `run_all()` iterates `sessions.json`, captures each live pane, returns JSON array
- CLI: `agent-doc prompt --all` (file arg now optional)
- 2 new unit tests (serialization, optional field omission)

**Phase 3: Plugin multi-session** — `PromptPoller.kt`
- Replaced single-file tracking with `ConcurrentHashMap<String, TrackedFile>`
- `addFile(file)` adds to tracked set, `ensurePolling()` starts poller once
- Polls via `agent-doc prompt --all` (one process per cycle)
- Auto-discovers sessions from `--all` output (VirtualFile resolution via `LocalFileSystem`)
- Each tracked file gets independent `modificationStamp` tracking

**Phase 4: File context in PromptPanel** — `PromptPanel.kt`
- `show()` accepts optional `fileName` parameter
- Prompt question shows `[plan.md] Do you want to proceed?`

**SubmitAction.kt** — changed `start(file)` → `addFile(file)` (additive, no stop)

### `agent-doc commit <file>` — new subcommand

Added `agent-doc commit <file>` which runs `git add -f <file>` + `git commit` with a generated `agent-doc: YYYY-MM-DD HH:MM:SS` message. No shell `$()` substitution needed — the timestamp is generated internally by the binary.

The `/agent-doc` skill can now use:
```
agent-doc commit tasks/plan.md
```
instead of:
```
git add -f tasks/plan.md && git commit -m "agent-doc: $(date '+%Y-%m-%d %H:%M:%S')" --no-verify
```

## User

PR, squash merge, release agent-doc

## Assistant

Done. agent-doc v0.3.0 released.

1. **PR** — [#5](https://github.com/btakita/agent-doc/pull/5) (multi-session prompt polling + commit subcommand)
2. **Squash merge** — merged to main as `0c352ed`
3. **Version bump** — v0.2.0 → v0.3.0 in `Cargo.toml`, changelog in `VERSIONS.md`
4. **crates.io** — [published](https://crates.io/crates/agent-doc/0.3.0)
5. **GitHub Release** — [v0.3.0](https://github.com/btakita/agent-doc/releases/tag/v0.3.0) with prebuilt `x86_64-unknown-linux-gnu` binary

87 tests passing. Includes the previously-unreleased prompt detection and send-keys fixes from v0.2.0.

## User

Is pypi updated as well?

One issue...the 0.2.0 release was skipped. Let's learn from this. Let's have AGENTS.md understand tha

## Assistant

**PyPI** — updated. `pyproject.toml` bumped from v0.1.4 → v0.3.0 and published via `maturin publish`. Both wheel (`manylinux_2_39_x86_64`) and source distribution uploaded.

PyPI was 3 versions behind because v0.2.0 was tagged but never published to PyPI (only crates.io + GitHub Releases). Now in sync.

On the v0.2.0 skip — waiting for you to finish your thought on AGENTS.md conventions.

## User

We can leave the 0.2.0 skip as is. Next time, I would rather change the unpublished 0.2.0 release to the head of main. So there would only be the 0.2.0 release.

Still getting this. Is agent-doc setup to fix this? Do we need to restart the claude session for the latest agent-doc update?

git add -f tasks/plan.md && git commit -m "agent-doc: $(date '+%Y-%m-%d %H:%M:%S')" --no-verify
Commit plan.md before responding

Command contains $() command substitution

Do you want to proceed?
❯ 1. Yes
2. No
