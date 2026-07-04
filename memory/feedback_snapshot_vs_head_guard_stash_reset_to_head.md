---
name: feedback_snapshot_vs_head_guard_stash_reset_to_head
description: "preflight \"snapshot differs from HEAD without an open or recoverable cycle\" on a managed-queue-only user edit — make snapshot==HEAD (not ==working-tree) so the edit becomes the normal diff"
metadata: 
  node_type: memory
  type: feedback
  originSessionId: 4ddb35c3-1ec8-4bb4-b6c6-f95aec82ff08
---

When `agent-doc preflight` fails step 0 with `snapshot differs from HEAD without an open or recoverable agent-doc cycle` and the only working-tree delta is a user edit to a managed component (e.g. an `agent:queue` go-queue head) plus an auto-recomputed `agent:boundary:<hash>`, the supervisor is fresh and there is no in-flight response — it's just a stale snapshot blocking the layout guard.

`reset --from-current --preserve-session` does NOT fix it: that sets snapshot == working-tree (still ≠ HEAD), so the guard persists. The guard wants snapshot == HEAD with the user edit living as the *detectable working-tree diff*.

**Fix:** make snapshot == HEAD while keeping the edit in the working tree:
1. `git stash push -- <FILE>` (working tree returns to HEAD)
2. `agent-doc reset --from-current --preserve-session <FILE>` (snapshot rebuilt from the now-clean HEAD)
3. `git stash pop` (restores the edit)

Then `preflight` diffs working-tree-edit vs snapshot(==HEAD) and processes the queue edit normally (`queue: head prompt modified but buffer settled — adopting edited head`).

**Why:** preflight interprets a user edit as the new prompt only when it's a diff against a HEAD-aligned snapshot. A stale snapshot ahead of HEAD trips the layout guard before the diff logic runs. `write --commit` won't help here — it refuses with "empty response — nothing to write" because there's no response body, only a managed-component baseline change.

Related: [[feedback_compaction_stale_snapshot_reset_not_commit]], [[feedback_supervisor_crdt_replay_reset_from_current]], [[feedback_ipc_crdt_response_loss_forcedisk_recovery]].
