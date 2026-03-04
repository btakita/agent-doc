---
agent_doc_session: cf853a21-3aaf-4775-9cc8-0e789109f9dd
tmux_session: '1'
---



# Plan: agent-doc & agent-loop

## Status

- **agent-doc**: v0.5.4 — crates.io + PyPI + GitHub Releases. 93 tests (72 unit + 21 integration).
  - Session routing: `start`, `route`, `claim`, `focus`, `layout`, `prompt`, `commit`
  - `claim`: `--position`, `--window`, `--pane` flags. Window-scoped routing via sessions.json.
  - `layout`: mirror editor splits, session-aware break-out, `--window` scoping
  - `prompt`: capture-pane polling, `--all`, `--answer <N>`. Plugin shows bottom-anchored panel.
  - `skill install/check`, `commit` (superproject-aware), upgrade self-update
- **corky**: v0.12.0 — crates.io + PyPI + GitHub Releases. 519 tests.
  - LinkedIn OAuth2, `_cmd` credential fields, contact sync v2, topics, scheduling
  - Gmail filters: `filter auth/pull/push/build`, `label clear`
  - `corky transcribe`: whisper-rs, feature-gated CUDA, speaker turn detection
- **instruction-files**: v0.1.3 — crates.io. 47 tests. SPEC.md discovery, line budget.
- **agent-doc-jetbrains**: v0.1.0 — signed plugin.
  - Run/Claim/Layout Sync hotkeys, Alt+Enter popup (ActionPromoter), PromptPoller/PromptPanel
  - Positional claim (Splitter tree walk), tab-to-pane sync (EditorTabSyncListener)
  - Window-scoped `--window` passthrough via `TerminalUtil.projectWindowId()`
- **agent-doc-vscode**: v0.1.0 — extension.
- **agent-loop**: Root Makefile (`release`, `update`, `check`, `precommit`, `publish`, `clean`).

### Session Routing (tmux)

```
agent-doc start <file>                 # register pane, exec claude
agent-doc route <file>                 # send-keys to pane, auto-start if needed
agent-doc claim <file> [--position] [--window] [--pane]
agent-doc focus <file> [--pane]        # focus tmux pane (cross-window)
agent-doc layout <files...> --split h|v [--window]
agent-doc prompt <file>                # detect permission prompts → JSON
agent-doc prompt --all                 # poll all live sessions
agent-doc prompt --answer <N> <file>   # answer a prompt
agent-doc commit <file>                # git add + commit with timestamp
agent-doc skill install|check          # SKILL.md management
```

## Outstanding

### agent-doc
- [ ] Codex backend (`src/agent/codex.rs`) — plan at `tasks/integrations/codex.md`
- [ ] `TmuxBatch` command batching — reduce tab-switch flicker

### instruction-files
- [ ] Collapse `AuditConfig` presets to single polyglot `Default`
- [ ] `.instruction-files.toml` project-root config file

### corky — Topics
- [ ] Extract 3-way merge to shared `context_sync` module
- [ ] Topic sync with eligibility check
- [ ] Thread tagging during sync

### corky — Integrations
- [ ] Telegram: `grammers-client` — plan at `corky/tasks/integrations/telegram.md`
- [ ] Slack: `slack-morphism` — plan at `corky/tasks/integrations/slack.md`

### Future
- [ ] Streaming responses, document-only permission flow, desktop notifications
- [ ] Multi-agent documents, email transport bridge with corky
- [ ] Personal data separation — nested git repo inside `tasks/`

## Design Decisions

- **session vs resume**: `session` = permanent routing UUID, `resume` = Claude conversation ID (cleared by reset)
- **instruction-files dependency**: `path + version` — local builds use path, `cargo publish` uses crates.io
- **tmux routing**: direct `send-keys -t <pane>`, not broadcast. Each doc has own UUID → pane mapping.
- **Prompt detection**: `tmux capture-pane` polling, ANSI strip, regex match. Fragile but works. Future: Claude Code hooks API.
- **Window-scoped routing**: `SessionEntry` stores `window` field. Plugin reads `projectWindowId()` from registry. Layout filters panes to claimed window; only breaks out registered session panes.
- **Topics config**: `[[topics]]` in `.corky.toml` with labels, contacts, `shared_with`.
- **Context routing**: Topics, contacts, threads share content-hash 3-way merge with eligibility rules.

## Reference

- `agent-doc/SPEC.md` — Decisions & architecture
- `agent-doc/CLAUDE.md` — Module layout & conventions
- `SPEC.md` — Workspace structure
- `CLAUDE.md` — Workflow orchestration

## Session Log

### Session 1 (2026-02-25) — Items 1-41

Releases: agent-doc v0.3.0 → v0.4.0 → v0.4.1 → v0.4.2, corky v0.11.1 → v0.11.3, instruction-files v0.1.1 → v0.1.2.

Key work: Topics (Phases 1-3), multi-session polling, `agent-doc commit/claim/skill`, PromptPanel (`JDialog` → `JLayeredPane` overlay), prompt cycling fix, SKILL.md comment stripping + version frontmatter, social credential `_cmd` fields, SPECS.md in audit-docs, WrapLayout for button overflow, `tmux-setup.sh`.

### Session 2 (2026-02-26) — Items 42-60

Releases: corky v0.11.2 → v0.11.3 → v0.11.4 → v0.11.5, instruction-files v0.1.2 → v0.1.3, agent-doc v0.4.3 → v0.4.4.

Key work: LinkedIn OAuth 401 fix, CWD-as-data-dir, watch Ctrl+C fix, plugin merge-safe reload (3-way merge), SPECS→SPEC rename (all 4 repos), PyPI catch-up, `corky label clear`, Gmail label cleanup (10,438 messages).

### Session 3 (2026-02-27) — Items 61-97

Releases: agent-doc v0.5.1, corky v0.12.0.

Key work: Gmail filters (`auth/pull/push/build`, config restructure), `corky transcribe` (whisper-rs, CUDA, speakers), plugin focus fix (Alt+Esc toggle, 1-9 selectors), dynamic plugin (no restart), CI fix, `agent-doc focus/layout` implemented, tab-to-pane sync, layout break-out + target window heuristics, editor plugin migration to `agent-doc/editors/`.

### Session 4 (2026-02-28) — Items 98-124+

Releases: agent-doc v0.5.2 → v0.5.3 → v0.5.4.

Key work: Claims notification log, positional claim (`--position` + Splitter tree walk), plugin logging cleanup → IntelliJ Logger, showHint for all actions, editor SPEC files, PID liveness (added then reverted), Alt+Enter fix (ActionPromoter `promote()` + self-disabling popup), "Replace with" diagnosed as IDEA bug. Window-scoped routing (`--window` on claim/layout, `SessionEntry.window`, `projectWindowId()`). PromptPoller `@Volatile` race fix. Layout session-aware break-out. PromptPanel truncation (45 chars) + height cap (30%).

### Session 5 (2026-03-03)

Key work: Yolo mode enabled (`bypassPermissions`). Token efficiency discussion (`smart_outline`/`smart_unfold`/`smart_search` from claude-mem plugin). Audit-docs run (fixed 4 stale agent-doc docs: SPEC.md, README.md, SKILL.md, CLAUDE.md). Submodule management: added briantakita.me → swapped to briantakita.me-dev → explored symlink/submodule emulation (git rejects symlinks for submodule paths, Gitman closest tool) → removed briantakita.me-dev submodule → replaced with gitignored `ctx-core-dev` symlink to `~/work/ctx-core/ctx-core-dev`. Committed all submodules (agent-doc, corky, philosophy, presence, resume) and task files.

## User

commit agent-loop and push
