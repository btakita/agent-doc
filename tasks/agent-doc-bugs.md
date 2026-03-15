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

### Re: Docs updated

All 4 stale docs updated. `audit-docs` now reports **0 issues** (377/1000 lines).

**Changes:**
- **SKILL.md** — Bumped version to 0.17.5, added IPC-first write bullets
- **CLAUDE.md** — Added IPC-first writes subsection, fixed code block warning (added imperative context), moved 14-row table to README
- **README.md** — Added IPC-first writes section + domain ontology table (from CLAUDE.md)
- **SPEC.md** — Added IPC-first behavior paragraph to section 7.21

Committed in agent-doc submodule (`a9d1a69`).

In rebuildjs-refactor.md, I requested that the Phases checkboxes are updated.
This sort of tracking state should always be updated via CRDT...To track ongoing progress.

Please add the relevant skills and implementation to agent-doc to ensure that fields are updated as state changes.
<!-- /agent:exchange -->
