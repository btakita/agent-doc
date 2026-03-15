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

### Re: tmux orientation — additional clue

Thanks for the extra detail. The behavior "switching editor focus to the right side reverts to horizontal" suggests the tmux layout is being disrupted by `agent-doc run`'s pane creation/resizing, not permanently changed. The `split-window` or `select-layout` command may be triggering a layout recalculation.

Fix agent is still investigating the source code. Will update with findings.
<!-- /agent:exchange -->
