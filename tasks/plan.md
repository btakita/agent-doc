---
session: cf853a21-3aaf-4775-9cc8-0e789109f9dd
agent: null
model: null
branch: null
---

# Plan: agent-doc & agent-loop

## Status

- **agent-doc**: v0.1.2 — crates.io + PyPI. 78 tests passing.
- **corky**: v0.9.1 — crates.io + PyPI. 237 tests passing. Contact sync v2 complete (unreleased).
  - Conversation-aware eligibility, explicit sharing, alias matching, 3-way merge via `.sync-state.json`
  - 26 contact sync tests (19 eligibility + 7 three-way merge)

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

Publish corky 0.9.1.
Commit and push mailboxes.
cleanup plan.md
squash merge in agent-loop PR branch
