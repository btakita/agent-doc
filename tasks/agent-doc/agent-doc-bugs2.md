---
agent_doc_session: b26b9957-8de6-4eae-bb98-5d7fe4d6b781
agent_doc_format: template
agent_doc_write: crdt
---

## Status

<!-- agent:status patch=replace -->
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
**Bugs fixed:** race condition from multiple stash panes claiming same doc; streaming checkpoint orphans; multi-session doc accumulation. **Safeguards implemented (all 4 layers):** L1 registry uniqueness (already done), L2 preflight CONFLICT hard-block, L3 cycle lock across full preflight→commit span, L4 orphaned checkpoint auto-discard. **git.rs change:** `agent-doc commit` now stages document file (HEAD stripped) so responses don't accumulate across sessions — blue gutter marks cycle boundary. **Files cleaned up:** agent-doc-bugs.md and claudescore.md committed.
Why?
```
● Bash(agent-doc preflight tasks/agent-doc/agent-doc-feat.md)
  ⎿  [preflight] step 0: layout check
     [preflight] layout issue: stash window 'stash' has non-idle pane %1 running 'agent-doc'
     [preflight] layout issue: stash window 'stash' has non-idle pane %36 running 'agent-doc'
```
<!-- /agent:exchange -->

## Pending / Not Built

<!-- agent:pending patch=replace -->

<!-- /agent:pending -->
