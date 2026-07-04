---
name: feedback_hung_pending_add_recover_via_agent_doc_commit
description: "a granular --pending-add that hangs on live-editor IPC still lands the item on disk; recover by committing via `agent-doc commit` (absorbs the pending-ID superset)"
metadata: 
  node_type: memory
  type: feedback
  originSessionId: 7b8e3c3a-f205-4401-8c0b-27c1e79f2001
---

`agent-doc write --pending-add ...` against a live CRDT/editor doc can hang on the editor IPC ack (timed out at 2 min). The patch is still **applied to the working tree** before the hang (doc grows by the item; only a benign `agent:boundary:<hash>` recompute alongside it), but the snapshot stays stale and HEAD does not get it. No stuck patch sidecar, no surviving process after the timeout.

**How to apply:** Do NOT re-run `--pending-add` (duplicates the item) and do NOT churn the IPC. Verify the item is in the working tree (`grep` the file) and that the working-tree diff vs HEAD is exactly that one item, then run `timeout 45 agent-doc commit <FILE>`. `git::commit()` absorbs an agent-owned **pending-ID superset** out-of-band drift into the snapshot and commits it ("[commit] absorbing out-of-band pending mutation into snapshot") even though the snapshot was behind — HEAD then has the item. Guard the commit with `timeout` so it can't re-hang. Related: [[feedback_degraded_ipc_blocks_granular_pending_writes]], [[feedback_hung_agent_doc_write_stale_live_buffer]].
