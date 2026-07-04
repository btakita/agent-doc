---
name: feedback_agent_doc_document_is_source_of_truth
description: "In agent-doc sessions the document/queue IS the authorization; drain go-mode queue, don't ask out-of-band"
metadata: 
  node_type: memory
  type: feedback
  originSessionId: 68d687b0-ffa3-42c3-a6e4-597f83aaa8c7
---

In an agent-doc session, the session **document is the source of truth**. When go-mode (`<!-- agent:queue ... go -->`) auto-pulls prioritized backlog items into the active queue, that queue IS the authorization to execute them — drain it in priority order. Do **not** stop and ask out-of-band (e.g. AskUserQuestion) whether to implement queued items.

**Why:** User directive 2026-06-06, answering a mid-session AskUserQuestion about whether to auto-implement go-mode-pulled `#rankdefault`/`#surrealbulk`: "Use the document as the source of truth." Earlier in the same session I had treated go-mode auto-pull of gated/recommended items as NOT authorization (removed them, waited for explicit "do") — that was over-cautious for ungated, prioritized backlog items under an active `go` queue.

**How to apply:** If the queue is `go`/active with prompts and `user_intent_prompt_changes` is empty, continue the auto-loop and implement the queued items per the normal cycle — full implementation + tests + build/install + commit/push + `--done`. Reserve gating/skipping for items that are genuinely blocked or explicitly gated `[/]`. Edit the document (queue/frontmatter `queue: stop`, remove `go`) to change flow, rather than asking in chat. Relates to [[feedback_session_document_scope_discipline]].
