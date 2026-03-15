---
agent_doc_session: 92fba2ac-9d1f-4a7a-8764-821a8a08ecd0
tmux_session: '0'
agent_doc_format: template
agent_doc_write: crdt
---

In tasks/briantakita.me.md, the CRDT created duplicate data. Please add tests to replicate. Fix bug.
```
Why were the videos not public to begin with?
Cork

Why were the videos not public to begin with?
Corky should always publish public videos...unless explicitly to private.
```

<!-- agent:exchange -->

### Re: Tracking state via CRDT

Good idea — auto-updating progress checkboxes as work completes. A few approaches:

**Option A: `<!-- agent:status -->` component with patch blocks**
The agent writes `<!-- patch:status -->` blocks during streaming to update a status component. The CRDT handles concurrent edits. This already works with the template mode — just needs the SKILL.md to instruct Claude to update status components as work progresses.

**Option B: Dedicated `agent-doc progress` command**
A CLI command that updates specific checkbox lines in a document:
```bash
agent-doc progress tasks/rebuild-refactor.md "Phase 1" --done
```
Would find `- [ ] Phase 1` and replace with `- [X] Phase 1`, writing via IPC.

**Option C: Frontmatter-based tracking**
Add a `progress` field to frontmatter that the agent updates:
```yaml
progress:
  phase_1: done
  phase_2: in_progress
  phase_3: pending
```

It looks like you were about to suggest something — what were you thinking with "Perhaps we want to use"?

In rebuildjs-refactor.md, I requested that the Phases checkboxes are updated.
This sort of tracking state should always be updated via CRDT...To track ongoing progress.

Please add the relevant skills and implementation to agent-doc to ensure that fields are updated as state changes.

Would reactive signals or lazily style slots + cells help?
Should we implement lazily-rs as a counterpart to btakita/lazily-py and btakita/lazily-zig?
<!-- /agent:exchange -->
