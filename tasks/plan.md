---
session: cf853a21-3aaf-4775-9cc8-0e789109f9dd
agent: null
model: null
branch: null
---

# Plan: agent-doc & agent-loop

## Status

- **agent-doc**: v0.4.0 — crates.io + PyPI + GitHub Releases. 93 tests passing (72 unit + 21 integration).
  - Session routing: `start`, `route`, `claim`, `prompt`, `commit` subcommands with auto-start cascade
  - `claim`: bind document to current tmux pane (last-call-wins, skill-invokable via `/agent-doc claim`)
  - `skill install` / `skill check`: embedded SKILL.md management via `include_str!`
  - `commit`: git root resolution (superproject-aware, no CWD drift issues)
  - Multi-session polling: `prompt --all`, plugin multi-file tracking
  - Prompt answering: `prompt --answer <N>` navigates + confirms via arrow keys
  - Upgrade check + self-update via GitHub Releases
- **corky**: v0.10.0 — crates.io + GitHub Releases. 290 tests passing.
  - Social media posting: LinkedIn OAuth2, draft creation/publishing
  - Contact sync v2: conversation-aware eligibility, 3-way merge
  - Topics: config, list, add, info, suggest
  - Scheduling: `corky schedule run/list`
- **instruction-files**: v0.1.0 — crates.io. 45 tests passing.
  - Shared audit crate: staleness, tree paths, line budget, actionable content checks
- **agent-doc-jetbrains**: v0.1.0 — signed plugin.
  - Hotkey (`Ctrl+Shift+Alt+A`) + right-click context menu
  - Multi-session PromptPoller with `prompt --all`, file-context in PromptPanel
  - PromptPanel: bottom-anchored bar with numbered hotkeys, IDE editor font scaling
- **agent-doc-vscode**: v0.1.0 — extension. Same hotkey, calls `agent-doc route`.
- **agent-loop**: Root Makefile with `make release`, `make update`, `make check`, `make precommit`, `make publish`, `make clean`.

### Session Routing (tmux)

```
agent-doc start <file>                 # register pane, exec claude
agent-doc route <file>                 # send-keys to pane, auto-start if needed
agent-doc claim <file>                 # bind file to current tmux pane
agent-doc prompt <file>                # detect permission prompts → JSON
agent-doc prompt --all                 # poll all live sessions
agent-doc prompt --answer <N> <file>   # answer a prompt
agent-doc commit <file>                # git add + commit with timestamp
agent-doc skill install                # write bundled SKILL.md to project
agent-doc skill check                  # compare installed vs binary version
```

## Outstanding

### agent-doc

- [ ] Codex backend (`src/agent/codex.rs`) — plan at `tasks/integrations/codex.md`

### agent-doc-jetbrains

- [x] PromptPanel focus fix — `JDialog` → `JLayeredPane` overlay (no WM popup leak, persists on focus change)

### instruction-files

- [ ] Collapse `AuditConfig` presets to single polyglot `Default` (always audit both AGENTS.md + CLAUDE.md)
- [ ] `.instruction-files.toml` project-root config file (override extensions, root markers, skip dirs)

### corky — Topics

- [ ] Extract core 3-way merge from `contact/sync.rs` → shared `context_sync` module
- [ ] Topic sync: eligibility check (label/contact match OR `shared_with`), delegate to shared merge
- [ ] Thread tagging: auto-add `**Topic**: {name}` to matching conversations during sync

### corky — Integrations

- [ ] Telegram: Live sync via `grammers-client` — plan at `corky/tasks/integrations/telegram.md`
- [ ] Slack: API sync via `slack-morphism` — plan at `corky/tasks/integrations/slack.md`

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

1. Pending releases audited — agent-doc v0.3.0 released (crates.io + PyPI + GitHub Release)
2. JetBrains PromptPanel font — scaled with IDE editor font
3. Topics implemented — Phases 1-3 complete (config, CLI, suggest). 290 corky tests passing.
4. PromptPoller fixes — sync refresh, one-shot reload via `modificationStamp`
5. Multi-session polling — 4-phase implementation (SessionEntry file → `--all` CLI → plugin multi-poll → file context)
6. `agent-doc commit <file>` — new subcommand, replaces `$()` shell substitution
7. `agent-doc claim <file>` — bind document to current tmux pane (last-call-wins)
8. `agent-doc skill install` / `check` — embedded SKILL.md management
9. `agent-doc commit` — git root resolution (superproject-aware)
10. v0.4.0 released — [PR #6](https://github.com/btakita/agent-doc/pull/6) squash merged, published to crates.io + PyPI + GitHub Release
11. PromptPanel fix — `JDialog` → `JLayeredPane` overlay (no WM leak, persists on focus change). Plugin rebuilt and signed.
12. Document cleanup — condensed from 685 → 150 lines

## User

I think agent-doc should ignore markdown comments. This would allow someone to plan out a multi-step set of interactions with the agent. wdyt?

## Assistant

Great idea. Markdown comments (`<!-- ... -->`) as a hidden scratchpad for planning multi-step interactions.

**Use case:**

```markdown
## User

Implement claim command

<!-- Step 2: Update SPECS.md with use case table -->
<!-- Step 3: PR, squash merge, release -->
```

After the agent responds to step 1, uncomment step 2 and submit. The agent sees it as new text.

**Recommendation: strip from diff only, keep in context.**

The agent should *see* comments (useful context), but changes to comments shouldn't trigger a response:
- Adding a comment = no diff (silent)
- Uncommenting text = diff detected (agent responds)
- Editing a comment = no diff (silent)

**Implementation:** Skill-level change in SKILL.md — strip `<!-- ... -->` blocks before comparing snapshot vs current. Snapshot stores full content, diff algorithm ignores comment-only changes.

Should I update the SKILL.md?

## User

yes

The commits were on a branch. Now I lost my changes. What branch was I on?

We absolutely need to maintain branch discipline. I was not aware that we were off of the main branch. Now I potentially lost a bunch of work.

I also need you to look at the reflogs because I reset the last commit.
