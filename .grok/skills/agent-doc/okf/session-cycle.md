---
concept_id: agent-doc.session-cycle
type: concept
tags: [agent-doc, session, closeout]
---

# Session Cycle

A session cycle is one binary-owned response unit for an agent-doc document:
preflight reads and repairs the current state, the agent responds to the
document prompt, and persistence commits the response through `finalize` or
`write --commit`.

The cycle is complete only when the binary records a committed closeout and
`session-check` accepts the resulting state. Console-only responses, direct
manual patchbacks, and tool-only authentication steps are not closeout
boundaries.

## Use When

Load this concept when reasoning about whether a turn is finished, whether a
response may be reported as complete, or whether repair must go through the
binary write path.
