---
name: feedback_agent_doc_batch_pending_into_response_cycle
description: "In a go-mode agent-doc queue, batch pending mutations INTO the response cycle; a separate commit consumes the next queue head with no response"
metadata: 
  node_type: memory
  type: feedback
  originSessionId: 0c07e6f3-57c8-42c7-951a-84253e8e36f3
---

In a go-mode `agent:queue` agent-doc session, every cycle-closing commit
(`finalize` OR `write --commit`) consumes exactly ONE queue head. So:

- **Do NOT run a separate `write --commit` after `finalize`** to land pending
  mutations (`--pending-add`/`--review-remove`/`--pending-gate`) — it closes a
  SECOND cycle and silently consumes the NEXT queue head with no response
  (churn; the struck item is unanswered). Observed 2026-06-12: a post-finalize
  `write --commit` for pending ops struck the next `:pushpin:` item.
- **Batch the response AND the pending mutations into ONE cycle:**
  `cat <<'R' | agent-doc write <FILE> --commit --force-disk --baseline-file <bf> --pending-gate <id> --origin skill` — `write --commit` takes the response body on stdin (template `patch:exchange`) AND pending flags together, commits once, consumes exactly the current head.
- **Never use bare `agent-doc write` (no `--commit`) to mutate pending mid-session** — it auto-commits pending mutations and then fails `session-check INTERRUPTED` on the open cycle (and can re-trigger the stale `live_prompt_drift` 13.5KB artifact). Recover via `git checkout HEAD -- FILE` → `reset --from-current --preserve-session` → re-run as one `write --commit --force-disk` cycle.

**Why:** the queue-consume-on-commit coupling means commit count must equal the
number of heads you intend to answer this turn.

**How to apply:** one commit per queue head; fold pending edits into the
response's `write --commit`/`finalize` call, using `--force-disk` whenever the
session is IPC-drift-prone. Related: [[feedback_ipc_drift_wedged_preparing_controllers]], [[feedback_ipc_crdt_response_loss_forcedisk_recovery]], [[feedback_operator_drive_items_on_go_queue_churn]].
