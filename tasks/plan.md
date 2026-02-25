---
session: cf853a21-3aaf-4775-9cc8-0e789109f9dd
agent: null
model: null
branch: null
---

# Plan: agent-doc & agent-loop

## Status

- **agent-doc**: v0.2.0 — crates.io + GitHub Releases. 85 tests passing (64 unit + 21 integration).
  - Tmux session routing: `start`, `route`, `prompt` subcommands with auto-start cascade
  - `Tmux` struct with `-L` socket isolation for testing (9 tmux isolation tests)
  - Session registry (`sessions.json`) — UUID → tmux pane mapping, liveness checks, auto-start
  - Prompt detection: `agent-doc prompt <file>` captures pane, parses Claude Code prompts → JSON
  - Prompt answering: `agent-doc prompt --answer <N> <file>` navigates + confirms via arrow keys
  - `send_keys` split: literal text (`-l`) + 50ms delay + separate Enter (Claude Code TUI fix)
  - `new_window -a`: append flag for next available window index
  - `AGENT_DOC_NO_AUTOSTART`: env var to prevent auto-start in tests
  - Upgrade check + self-update via GitHub Releases
  - Auto-frontmatter: `ensure_session()` generates UUID v4 for documents without one
  - Separated `session` (routing UUID) from `resume` (Claude conversation ID) in frontmatter
- **corky**: v0.10.0 — crates.io + GitHub Releases. 220 tests passing.
  - Social media posting: LinkedIn OAuth2, draft creation/publishing, 6 CLI subcommands, 54 social-specific tests
  - Contact sync v2: conversation-aware eligibility, explicit sharing, alias matching, 3-way merge
  - Upgrade check + self-update via GitHub Releases
- **instruction-files**: v0.1.0 — crates.io. 45 tests passing.
  - Shared audit crate: staleness, tree paths, line budget, actionable content checks
  - `AuditConfig::agent_doc()` / `AuditConfig::corky()` presets (planned: collapse to single polyglot default)
  - README with usage, config reference, consumer links
- **agent-doc-jetbrains**: v0.1.0 — signed plugin.
  - Hotkey (`Ctrl+Shift+Alt+A`) + right-click context menu
  - `SubmitAction`: saves all documents before routing, starts prompt poller after submit
  - `TerminalUtil`: `resolveAgentDoc()` checks `~/bin`, `~/.local/bin`, `~/.cargo/bin`, `/usr/local/bin`
  - `PromptPoller`: polls `agent-doc prompt` every 1.5s after submit, refreshes VirtualFile from disk
  - `PromptPanel`: bottom-anchored bar with numbered hotkeys (1-9, Esc to dismiss)
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
- [ ] Codex backend (`src/agent/codex.rs`) — plan at `tasks/integrations/codex.md`

### instruction-files

- [ ] Collapse `AuditConfig` presets to single polyglot `Default` (always audit both AGENTS.md + CLAUDE.md)
- [ ] `.instruction-files.toml` project-root config file (override extensions, root markers, skip dirs)

### corky — Topics

- [ ] `TopicConfig` struct + wire into `CorkyConfig` deserialization
- [ ] Extract core 3-way merge from `contact/sync.rs` → shared `context_sync` module
- [ ] Topic sync: eligibility check (label/contact match OR `shared_with`), delegate to shared merge
- [ ] Thread tagging: auto-add `**Topic**: {name}` to matching conversations during sync
- [ ] `corky topics suggest` — scan untagged threads, propose assignments
- [ ] `corky topics list` — show topics with sync status per mailbox

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

## Assistant

Cleaned up. Rolled file-refresh fix into Status (PromptPoller now refreshes VirtualFile from disk). Topics: not implemented — all items remain in Outstanding.

## User

Show me the plan to implement topics.

## Assistant

### Topics Implementation Plan

**Phase 1: Config** — `TopicConfig` in `.corky.toml`
```toml
[topics.project-alpha]
keywords = ["alpha", "release", "milestone"]
mailboxes = ["work"]
contacts = ["alice", "bob"]
description = "Project Alpha development and releases"
```
- `TopicConfig` struct in `src/config/topic.rs` (parallel to `contact.rs`)
- `pub topics: HashMap<String, TopicConfig>` added to `CorkyConfig`
- Format-preserving writes via `toml_edit`

**Phase 2: CLI subcommands**
```
corky topics list [--verbose]                     # show configured topics
corky topics add <name> [--keyword K]             # add topic to .corky.toml
corky topics info <name>                          # show topic details + matching threads
corky topics suggest [--limit N] [--mailbox M]    # auto-discover from conversations
```
- `TopicsCommands` enum in `cli.rs` (mirrors `ContactCommands`)
- Each subcommand in `src/topics/{list,add,info,suggest}.rs`

**Phase 3: Topic suggestion**
- Scan `conversations/` (or `mailboxes/{name}/conversations/` if `--mailbox`)
- Extract thread subjects, strip `Re:`/`Fwd:` prefixes via existing `thread_key_from_subject()`
- Frequency histogram, return top N
- Simple and deterministic — no ML

**Phase 4: Thread tagging** (future, not in initial scope)
- During `corky sync`, auto-tag threads matching topic keywords
- Add `**Topic**: {name}` to conversation metadata
- Requires sync state extension

**Not in scope initially:**
- 3-way merge extraction — defer until topics become versioned documents
- Topics are static config, not synced files — no sync state needed
- Thread tagging during sync (Phase 4)

**Module layout:**
```
src/topics/
  mod.rs        # pub mod suggest, list, add, info
  suggest.rs    # frequency-based subject extraction
  list.rs       # list configured topics
  add.rs        # edit .corky.toml
  info.rs       # show topic details
src/config/
  topic.rs      # TopicConfig struct, load/save
```

## User

First...are there any pending releases?

How do I use the shortcut key to answer yes?

## Assistant

### Pending Releases

- **agent-doc**: 2 unreleased commits since v0.2.0:
  - `a07efe7` feat: add prompt detection subcommand
  - `56a5d6f` fix: send-keys split, new-window append, test isolation
  - Ready for v0.2.1 (patch) or v0.3.0 (minor, since prompt is a new feature).
- **corky**: Up to date — no unreleased commits since latest tag.
- **instruction-files**: Published to crates.io as v0.1.0 but has **no git tags**. Needs `git tag v0.1.0` on the publish commit.

### Shortcut Keys

In the JetBrains PromptPanel (the yellow bar at the bottom):

- Press **`1`** on your keyboard → selects "Yes"
- Press **`2`** → selects "No"
- Press **`Esc`** → dismisses the panel without answering

The numbered buttons `[1] Yes`, `[2] No` map directly to the number keys. The panel needs keyboard focus — if the number keys don't respond, click the yellow bar first, then press the number.

## User

How can I make the font size bigger for that dialog?

## Assistant

Done — bumped the font sizes in `PromptPanel.kt` and rebuilt the plugin:

| Element | Before | After |
|---------|--------|-------|
| Question | 12pt bold | **14pt bold** |
| Buttons | 11pt | **13pt** |
| Esc hint | 10pt italic | **11pt italic** |

Plugin rebuilt and signed. Restart IntelliJ to pick up the change (or re-install from `build/distributions/`).

If you want even larger, just say — or I can wire it to follow the IDE's editor font size via `EditorColorsManager` so it scales automatically with your preferences.

## User

Yes...scale it with the IDE editor font size

## Assistant

Done — `PromptPanel.kt` now reads the editor font size from `EditorColorsManager.getInstance().globalScheme.editorFontSize` and derives all panel fonts from it:

- **Question**: editor font size + 1, bold
- **Buttons**: editor font size
- **Esc hint**: editor font size - 2, italic

No more hardcoded pt values. The panel scales automatically when you change your editor font in **Settings > Editor > Font**. Plugin rebuilt and signed — restart IntelliJ to pick it up.

## User

Ok...now topics...I have another agent planning email + social media scheduling. Would the work on topics clash with the scheduling work?

## Assistant

**No clash.** The three features are architecturally isolated in corky:

| Feature | Config | Storage | Code modules |
|---------|--------|---------|--------------|
| Topics | `[topics.*]` in `.corky.toml` | Config-driven, no files | `src/topics/`, `src/config/topic.rs` |
| Social scheduling | `scheduled_at` in draft frontmatter | `mail/social/*.md` | `src/social/` |
| Email scheduling | Draft send times | `mail/drafts/*.md` | `src/draft/`, `src/sync/` |

Notable: `SocialDraftMeta` already has `scheduled_at: Option<DateTime<Utc>>` — it's just unused (`None`). The scheduling agent can populate that field and add a scheduler daemon entirely within `src/social/` without touching topics.

**Only 3 files touched by both** (trivial additions — enum variant + config field):
- `src/config/corky_config.rs` — `TopicConfig` parallel to existing `SocialConfig`
- `src/cli.rs` — `Topics(TopicCommands)` parallel to existing `Social(SocialCommands)`
- `src/main.rs` — one match arm

**Safe to work in parallel.** The other agent can focus on scheduling while topics goes into its own module.

## User

implement topics

## Assistant

Done — topics implemented (Phases 1-3). All 290 tests passing, clippy clean.

**Files created:**
- `src/config/topic.rs` — `TopicConfig` struct, `load_topics()`, `save_topic()` (format-preserving), 5 unit tests
- `src/topics/mod.rs` — module with `run_list`, `run_add`, `run_info`, `run_suggest` dispatch
- `src/topics/list.rs` — list configured topics (compact or `--verbose`)
- `src/topics/add.rs` — add topic to `.corky.toml`
- `src/topics/info.rs` — show details + matching conversations by keyword
- `src/topics/suggest.rs` — frequency histogram of conversation subjects, 3 unit tests

**Files modified:**
- `src/config/mod.rs` — added `pub mod topic`
- `src/config/corky_config.rs` — added `topics: HashMap<String, TopicConfig>` to `CorkyConfig`
- `src/lib.rs` — added `pub mod topics`
- `src/cli.rs` — added `Topics(TopicCommands)` with `List`, `Add`, `Info`, `Suggest` subcommands
- `src/main.rs` — added dispatch for all 4 topic subcommands

**CLI:**
```
corky topics list [--verbose]
corky topics add <name> --keyword kw1,kw2 [--description "..."]
corky topics info <name>
corky topics suggest [--limit N] [--mailbox NAME]
```

**No conflicts** with the scheduling agent's concurrent work (`Schedule` commands + `src/schedule/` module coexist cleanly).

## User

The document did not refresh. I had to run the Reload All from Disk / Reload command to see the new content.

## Assistant

That's expected when submitting from the Claude Code terminal (`/agent-doc`) rather than the JetBrains plugin.

The `PromptPoller` (which calls `file.refresh()` every 1.5s) only starts when you submit via **`Ctrl+Shift+Alt+A`** in JetBrains. The terminal `/agent-doc` workflow bypasses the plugin entirely, so the poller never starts and JetBrains doesn't know the file changed.

**Options:**
1. **Submit from JetBrains** (`Ctrl+Shift+Alt+A`) — the poller refreshes the file automatically every 1.5s
2. **JetBrains auto-sync on focus** — Settings > Appearance & Behavior > System Settings > "Synchronize files on frame activation" (on by default). Just alt-tab back to IntelliJ.
3. **Manual** — `Ctrl+Alt+Y` (Reload All from Disk), which is what you did

For the best document-editing experience, submit from JetBrains and let the poller handle refreshes.

## User

I did submit from JetBrains...but it doesn't seem to poll at times. I'm switching panes and selecting the terminal while the task is running.

Also, when I selected this plan.md, the resume.md had a question that was not picked up by the plugin.

## Assistant

Two issues, two root causes:

**1. File not refreshing** — Fixed. The poller was using `file.refresh(true, false)` (async), which batches VFS refreshes and may not trigger promptly when the editor loses focus. Changed to:
- `file.refresh(false, false)` — synchronous, completes immediately
- `FileDocumentManager.getInstance().reloadFiles(file)` — explicitly reloads the editor buffer on the EDT

Plugin rebuilt and signed. Restart IntelliJ to pick it up.

**2. resume.md prompt not detected** — Design limitation. The `PromptPoller` only monitors the **most recently submitted file**. When you submitted plan.md, the poller switched to polling plan.md's tmux session. resume.md's session prompt was invisible.

To fix this properly, the poller would need to monitor **all active sessions** (iterate `sessions.json` entries, poll each). That's a larger change — adding to Outstanding:
- `PromptPoller`: poll all active sessions, not just the last-submitted file

let's plan implementation for polling all active sessions.

## Assistant

### Multi-Session Prompt Polling Plan

**Problem:** `PromptPoller` tracks one file. Submitting plan.md stops watching resume.md. Permission prompts on other sessions go unnoticed.

**Approach:** Add `agent-doc prompt --all` (CLI-side iteration) so the plugin makes one call per poll cycle instead of N.

#### Phase 1: Store file path in SessionEntry

`sessions.rs` — add `file` field to `SessionEntry`:
```rust
pub struct SessionEntry {
    pub pane: String,
    pub pid: u32,
    pub cwd: String,
    pub started: String,
    pub file: String,  // NEW: relative path to session document
}
```
Set during `register()` in `start.rs`. Existing entries without `file` get `""` (serde default).

#### Phase 2: `agent-doc prompt --all`

`prompt.rs` — new mode that iterates `sessions.json`:
```
agent-doc prompt --all
```
Output:
```json
[
  {"session_id":"abc...","file":"tasks/plan.md","active":true,"question":"...","options":[...]},
  {"session_id":"def...","file":"tasks/resume.md","active":false}
]
```
- Load registry, filter to live panes only
- Capture each pane, parse for prompts
- Return JSON array

#### Phase 3: Plugin — poll all sessions

`PromptPoller.kt` changes:
- Remove single-file tracking (`currentFile`, `currentPrompt`)
- Track a `Set<VirtualFile>` of submitted files (add on each submit, never clear)
- Poll via `agent-doc prompt --all` instead of per-file
- Refresh all tracked VirtualFiles each cycle
- For each active prompt, show `PromptPanel` with file name in the question

`PromptPanel.kt` changes:
- Prepend file name to question: `"[resume.md] Do you want to proceed?"`
- If multiple prompts are active simultaneously, show them stacked or cycle between them

#### Phase 4: File refresh for all tracked files

Each poll cycle:
1. Call `agent-doc prompt --all` (one process)
2. For each tracked VirtualFile: `file.refresh(false, false)` + `reloadFiles(file)`
3. For each active prompt result: update PromptPanel

**Files changed:**

| File | Change |
|------|--------|
| `agent-doc/src/sessions.rs` | Add `file: String` to `SessionEntry` |
| `agent-doc/src/start.rs` | Pass file path to `register()` |
| `agent-doc/src/prompt.rs` | Add `--all` mode, `PromptAllInfo` struct |
| `agent-doc/src/main.rs` | Wire `--all` flag |
| `PromptPoller.kt` | Multi-session polling, tracked file set |
| `PromptPanel.kt` | Show file name, handle multiple prompts |

**Not in scope:**
- Auto-discovering files (only files submitted via plugin are tracked)
- Persisting tracked files across IDE restarts

## User

When I edit resume.md, the background loop reverts to the last saved state...unless I quickly save. Please only apply the sync **one time after the write happens**.
