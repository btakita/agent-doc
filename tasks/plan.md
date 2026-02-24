---
session: cf853a21-3aaf-4775-9cc8-0e789109f9dd
agent: null
model: null
branch: null
---

# Plan: agent-doc & agent-loop

## Status

- **agent-doc**: v0.1.3 — crates.io + PyPI. 88 tests passing.
  - Upgrade check: crates.io query with 24h cache, startup warning, `agent-doc upgrade` subcommand
- **corky**: v0.9.3 — crates.io + PyPI. 244 tests passing.
  - Contact sync v2: conversation-aware eligibility, explicit sharing, alias matching, 3-way merge via `.sync-state.json`
  - Upgrade check: crates.io query with 24h cache, startup warning, `corky upgrade` subcommand

## Outstanding

### agent-doc

- [ ] Codex backend (`src/agent/codex.rs`) — plan at `tasks/integrations/codex.md`

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

## Reference

- **Decisions & architecture:** `agent-doc/SPECS.md`
- **Module layout & conventions:** `agent-doc/CLAUDE.md`
- **Workspace structure:** `SPECS.md`
- **Workflow orchestration:** `CLAUDE.md`

## User

agent-doc & corky changes run in separate threads.

create PR branch for agent-doc & corky.
Provide auto-upgrade if possible. Otherwise warn user that an upgrade is available.

Write tests for new feature.

commit both
push both

squash merge PRs

increment patch version
push tag
publish

## Assistant

Done. Both projects got upgrade check + auto-upgrade in parallel:

**Implementation** (both `src/upgrade.rs`):
- Startup: `warn_if_outdated()` queries crates.io API (`/api/v1/crates/{name}`) via `ureq`, 5s timeout, 24h file cache (`~/.cache/{name}/version-cache.json`). Prints yellow stderr warning if outdated. All errors silently swallowed.
- `upgrade` subcommand: fresh version check, then tries `cargo install` -> `pip install --upgrade` -> manual instructions.
- Version comparison: simple `X.Y.Z` tuple ordering.

**Tests**: agent-doc 88 tests (10 new), corky 244 tests (7 new). Clippy clean.

**Released**:
- agent-doc v0.1.3 — crates.io + PyPI
- corky v0.9.3 — crates.io + PyPI

PRs: agent-doc #2, corky #7 (both squash-merged to main, tagged, published).

## User

What is the developer experience to upgrade the binary curl | sh...they would have to run the command again? What is typical for this sort of install? Is there an upgrade command that is often run?
