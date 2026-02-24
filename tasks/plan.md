---
session: cf853a21-3aaf-4775-9cc8-0e789109f9dd
agent: null
model: null
branch: null
---

# Plan: agent-doc & agent-loop

## Status

- **agent-doc**: v0.1.4 — crates.io + PyPI. 90 tests passing.
  - Upgrade check + self-update: crates.io query with 24h cache, startup warning, `agent-doc upgrade` with GitHub Releases download
- **corky**: v0.9.4 — crates.io + PyPI. 216 tests passing.
  - Contact sync v2: conversation-aware eligibility, explicit sharing, alias matching, 3-way merge via `.sync-state.json`
  - Upgrade check + self-update: crates.io query with 24h cache, startup warning, `corky upgrade` with GitHub Releases download

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

### Personal Data
Should I comingle by person communication with development in this repo?
I'm mainly concerned about the artifacts of my development. Like I don't want my personal communication to be in the commit history. But I'm using commits to iterate on plan.md.

Can we explore other options for separating personal communication from development artifacts?
Would a sidecar git repo be a good solution?

### Topics
I want to have topics that can be shared with collaborators. Communication can be assigned to topics as well via rules. A topic is a way of grouping. Other ways of groupings are labels and contacts.

A topic will have it's own AGENTS.md->CLAUDE.md/CLAUDE.local.md filespace. Other context files may be in there as well.

There needs to be a way to group topics with threads. Perhaps the frontmatter of the thread can contain the topic?

If a topic is missing, then the topic can be auto-created with suggested after a pass through the emails.

Each thread implicitly has it's own topic as well. So there should be an easy way to add context to the thread. Perhaps a context file can be created for thread as needed? Two options I see:

1. The filename can have a suffix like `.context.md`
2. A subdirectory with the <thread basename>/AGENTS.md file can be created.

I suppose the latter is more flexible, but not as simple & convenient. How about we start off with `.context.md` for now 

This also means collaborator threads, including topics, need to be synced across all collaborators.

### Context Routing with collaborators
I need a way for context to be routed bi-directionally with collaborators. The existing solution on the contact of the 3 way merge of AGENTS.md is sufficient for topics. I mainly need this process to be a central part of the collaboration workflow. So topics, contacts, and threads would all share this.
