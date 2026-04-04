---
agent_doc_session: 30b44f09-0517-4027-a91e-269cd59d4677
tmux_session: '0'
agent_doc_format: template
agent_doc_write: crdt
---

## Status

<!-- agent:status patch=replace -->
Queue initialized. 2 pending items. Awaiting task selection.
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->

### Agent (initial review)

**Pending items:**
1. **install nvidia_greenboost** — ready to pick up. What's the install method? (pip, cargo, apt?)
2. **agent-doc task queue** — the design below is solid

**Task Queue Design feedback:**

The MVP approach is smart — use existing primitives (`&lt;!-- agent:pending --&gt;` as queue, tmux-router for pane spawning, watch-mode for auto-submit) before building new binary features.

Progression:
- **Phase 1 (MVP):** SKILL.md workflow only. Orchestrator is a Claude Code session watching the queue doc, spawning tmux panes with `agent-doc claim`, monitoring `&lt;!-- agent:status --&gt;` in task docs.
- **Phase 2:** Queue semantics in agent-doc binary — task assignment locking, completion detection, cross-document signaling.
- **Phase 3:** Full orchestrator mode — `agent-doc orchestrate queue.md`.

**Open questions:**
- Task assignment locking: flock or claims log?
- Completion signaling: status component value, or removal from pending?
- Git conflicts: do parallel agents need isolated branches/worktrees?
<!-- agent:boundary:d0989227 -->
<!-- /agent:exchange -->

## Pending / Not Built

<!-- agent:pending patch=replace -->
- install nvidia_greenboost
- **agent-doc task queue** — orchestrator document + task assignment + parallel agent spawning (see below)
<!-- /agent:pending -->

## Ideas

### agent-doc Task Queue Model

Combine a centralized task queue with agent-doc's document-as-UI. Each task gets a document. The queue coordinates which agent works on which document.

**Design:**

1. **Queue document** — template-mode document with `<!-- agent:queue -->` listing tasks:
   ```markdown
   <!-- agent:queue -->
   - [ ] Fix auth middleware → tasks/auth-fix.md (assigned: pane %5)
   - [ ] Add pagination → tasks/pagination.md (unassigned)
   - [x] Update README → tasks/readme.md (done)
   <!-- /agent:queue -->
   ```

2. **Task documents** — each task is a regular agent-doc session. Progress visible in real-time.

3. **Orchestrator** — watch-mode agent monitors queue, spawns panes (via tmux-router), claims task documents. Updates queue on completion.

**What exists:**
- `parallel.rs` + `worktree.rs` — git worktree isolation
- `tmux-router` — pane management, routing, auto-start
- `sessions.json` — session registry
- `watch.rs` — file-watching for auto-submit
- Template mode + components — structured updates

**What's missing:**
- Queue component type with task assignment semantics
- Orchestrator that reads queue and spawns agents
- Task completion detection (via `<!-- agent:status -->` in task docs)
- Cross-document coordination (one agent's output → another's input)

**MVP:** Use `<!-- agent:pending -->` as task queue. Orchestrator document lists tasks. Watch-mode agent picks up unassigned tasks, creates panes, claims documents. No new binary features — just a SKILL.md workflow.

**Inspiration:** [ClawTeam](https://github.com/HKUDS/ClawTeam) — agent swarm orchestration with git worktrees + tmux, but task-queue-based rather than document-based.
