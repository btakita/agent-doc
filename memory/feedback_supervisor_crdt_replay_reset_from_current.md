---
name: feedback_supervisor_crdt_replay_reset_from_current
description: "When the live supervisor re-commits a corrupted in-memory CRDT snapshot and git checkout HEAD won't hold, escalate to `reset --from-current`"
metadata: 
  node_type: memory
  type: feedback
  originSessionId: e9a3edfa-a1bc-4055-a426-63e6b9ef5984
---

In a degraded agent-doc session the route-owned in-process supervisor can replay a **stale corrupted in-memory CRDT** (`.agent-doc/crdt/<pathhash>.overlay.yrs`): after every clean commit you make, it re-commits its own snapshot — dropping review items, flipping `queue: stop`→`start`, and re-injecting backlog→queue churn (a `priority queue` backlog attr auto-syncs backlog ids into the queue). `git checkout HEAD -- FILE` does NOT hold here because the supervisor immediately re-asserts from memory.

**Why:** the corruption lives in the supervisor's in-memory CRDT, not just on disk; disk edits and git commits get overwritten. Symptom: repeated `postcommit_worktree_check match=false` + a recurring stale `patch_id` in `ipc_proof_insufficient` diagnostics + the supervisor authoring its own `agent-doc(...)` commit seconds after yours.

**How to apply (escalation ladder):**
1. `git checkout HEAD -- FILE` for a one-off post-commit worktree drift (see [[feedback_post_commit_worktree_drift_checkout_head_recovery]]).
2. If the supervisor RE-COMMITS corruption after that, stop fighting it: restore clean content into the working tree (`git checkout <good-sha> -- FILE`), then run **`agent-doc reset --from-current FILE`** — it "rebuilds snapshot/CRDT from current file", clearing the corrupt in-memory state. Then `agent-doc commit FILE`. The commit then holds with NO postcommit-corruption warning and session-check goes `committed` / `queue_continuation_required=false`.
3. If even reset won't hold, hand off: operator restarts the doc's session (kills the supervisor pid) — don't chase it. See [[feedback_agent_doc_supervisor_self_race_finalize_block]].

Two gotchas hit the same session (filed as agent-doc-bug `#opsproofgate`):
- **Commit-hash in a live-verify GATE's review text → false auto-completion.** Preflight's ops-proof scanner auto-completes a `[/]` review item on `evidence=commit` when the text references a submodule/commit hash (`auto_complete_ops_proof id=... evidence=commit`), archiving an UNDRIVEN operator-drive gate to done. Keep live-verify gate text **hash-free**; make the arbiter the anchored structured `^[epoch] <marker>` ops.log line, not a referenced commit.
- **Re-adding an id that's in the done archive gets reaped** as an "already-done mirror" (`review: reaped 1 already-done mirror item(s)`). Remove the false entry from `tasks/*.done.md` and commit that first, THEN `--review-add`/`--pending-add` the id.

Context: agent-doc-bugs2.md 2026-06-10, restoring operator-drive gates `#ktw8`/`#lvbremain` to review. Related [[feedback_operator_drive_items_on_go_queue_churn]], [[feedback_agent_doc_dual_patch_status_exchange_interleave]].
