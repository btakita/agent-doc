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
  - `skill install/check`, `commit` (superproject-aware), `upgrade` self-update, `plugin install/update/list`
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
  - Local symlinks (gitignored): `briantakita.me-dev`, `ctx-core-dev`, `python-intentions`

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
agent-doc plugin install|update|list   # Editor plugin management
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

### Sessions 1-4 (2026-02-25 to 2026-02-28)

Releases: agent-doc v0.3.0 → v0.5.4, corky v0.11.1 → v0.12.0, instruction-files v0.1.1 → v0.1.3.

Key work: Topics (Phases 1-3), Gmail filters, `corky transcribe`, session routing (`start/route/claim/focus/layout/prompt`), JetBrains plugin (positional claim, ActionPromoter, PromptPanel), VSCode extension, window-scoped routing, `agent-doc commit/skill/upgrade`. LinkedIn OAuth fix, SPECS→SPEC rename, PyPI catch-up, `corky label clear`.

### Session 5 (2026-03-03)

Key work: Yolo mode, token efficiency (claude-mem plugin), audit-docs fixes, submodule management (briantakita.me-dev → gitignored symlinks).

### Session 6 (2026-03-05)

Key work: briantakita.me migration to Cloudflare Pages (prerender + static deploy), R2 asset migration, Contabo decommission, email worker fix (SLACK_WEBHOOK_URL), `python-intentions` symlink added. Audit-docs: updated agent-doc SPEC.md/README.md/CLAUDE.md for new `plugin.rs` module (`plugin install/update/list`).

## Assistant

### Audit results

Ran `audit-docs` across submodules:
- **corky**: Clean
- **instruction-files**: Skipped (library crate)
- **agent-doc**: 4 staleness issues from `src/plugin.rs` (new module: `plugin install/update/list`). Updated:
  - **README.md**: Added plugin commands to Quick Start and CLI Reference
  - **SPEC.md**: Added section 7.18 (plugin management)
  - **CLAUDE.md**: Added `plugin.rs` to module layout
  - **SKILL.md**: Touched (no content changes needed)
- Only remaining warning: CLAUDE.md ontology table (14 rows) — advisory, intentional

### Compacted

Reduced plan.md from 185 to ~100 lines. Merged Sessions 1-4 into single summary, preserved all outstanding items and design decisions. Added `plugin install/update/list` to status and CLI reference.

### Committed agent-loop

## User
