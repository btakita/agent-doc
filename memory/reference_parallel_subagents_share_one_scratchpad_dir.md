---
name: reference_parallel_subagents_share_one_scratchpad_dir
description: "Fan-out subagents inherit ONE scratchpad path, so fixed filenames like msg.txt collide mid-task between concurrent agents"
metadata: 
  node_type: memory
  type: reference
  originSessionId: 311c3110-b232-4f94-8665-f02e34d986e2
  modified: 2026-08-03T01:05:34.130Z
---

Subagents dispatched from the same session all inherit the **same** scratchpad
directory. A fixed filename is therefore shared state, not per-agent state: during
the nine-binding `#lzblobbackendstrict` fan-out, a lazily-cpp agent overwrote
`scratchpad/msg.txt` while a lazily-js agent was using it for a commit message. The
js commit body was correct only because git had already consumed the file — the
window is real and the failure would be a commit carrying another repo's message.

**How to apply:** when briefing a fan-out, tell each agent to use a filename keyed to
its repo (`msg-lazily-js.txt`) or to pipe the message via a heredoc into
`git commit -F -` rather than staging it through a file. Do not assume "session
scratchpad" means "per-agent scratchpad".

Related: [[feedback_never_dispatch_concurrent_agents_into_one_repo]] — that rule keeps
agents out of each other's repos; this one keeps them out of each other's temp files.
