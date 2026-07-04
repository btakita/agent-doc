---
name: feedback_preflight_repair_consumes_queue_question_no_answer
description: agent-doc preflight repair can mark a queued question consumed via a stale orphan without producing any answer; verify the answer exists before trusting no_changes
metadata: 
  node_type: memory
  type: feedback
  originSessionId: 7b197f8e-5fc2-40ce-98f5-59860c8fba3e
---

In a degraded `/agent-doc` session, `preflight` step-1 repair can find a stale "orphaned response" (e.g. 1804 bytes), apply it, and report `[queue] consumed: "<the user's queued question>"` + `[queue] drained` + final `no_changes: true` — **even though no actual answer to that question exists anywhere in the exchange**. The repair matched the orphan to the queue head and struck it logically (remaining: 0), but the visible queue block still shows the question with its `go` token unstruck, and the exchange has no response.

**Why:** orphan-recovery + compaction + the prior cycle's IPC `content_ours` drift can desync the binary's queue-consumption state from the visible document. `no_changes:true` then reflects disk==snapshot after recovery, NOT that the question was answered.

**How to apply:** do NOT stop on `no_changes:true` + `queue drained` when a queued user question has no visible answer in the exchange. Grep the committed doc for an actual `### Re:` answer to that question; if absent, answer it yourself and persist via `agent-doc write --commit` (the prompt is already in the doc, so this is repair-path, not a fresh cycle). The leftover queue line with a stale `go` token while frontmatter is `queue: stop` is benign `inactive_queue_residue` — there is no CLI strike flag; leave it rather than risk a manual Edit that re-triggers supervisor/IPC drift. Related: [[feedback_ipc_crdt_response_loss_forcedisk_recovery]], [[feedback_agent_doc_stale_capture_blocks_commit]], [[feedback_post_commit_worktree_drift_checkout_head_recovery]].
