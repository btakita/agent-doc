---
name: feedback_forcedisk_committed_but_false_closeout_error_stop_retrying
description: "force-disk finalize can COMMIT to HEAD yet print \"refusing already-committed closeout\"; retrying + reset re-poisons baseline into an infinite dedup loop"
metadata: 
  node_type: memory
  type: feedback
  originSessionId: 77ba94e1-d85f-4b69-930f-0ab0226941cf
---

A stale JB plugin corrupted IPC writes (spliced two bullet lines from the response into the MIDDLE of the prior `### Re:` body + rewrote the boundary hash, instead of appending). `finalize`/`write --commit` then hit `IPC patch consumed but response content not found in file`. Recovery via `finalize --force-disk` (operator-authorized).

**The trap:** the FIRST `--force-disk` actually applied the response to disk AND committed it (a new `agent-doc(reitrades)` commit landed in HEAD, 46583B, single clean copy) — but it *also* printed `Error: captured response body is not present in the staged snapshot ... refusing already-committed closeout`. That error is a **false negative**. Then `reset --from-current` rebuilt the baseline+snapshot+CRDT *from the now-written doc*, so the baseline gained the response → every subsequent `finalize --force-disk` deduped (`no changes after merge, skipping write`) and re-emitted the same error. I burned several cycles clearing CRDT/capture sidecars chasing a doc that was already correct in git.

**Why:** `--force-disk` writes+commits before the closeout accounting; the dedup/`response_sha256` cycle-state then trips on the already-written state and leaves a stuck `synthetic-*` cycle in phase `write_applied`/`write_stream_dedup` that preflight cannot self-clear.

**How to apply:** on ANY `refusing already-committed closeout` after a `--force-disk`, **git is authoritative — check it FIRST**: `git show HEAD:<doc> | grep -c '<response marker>'` and `git log --oneline`. If the response is in HEAD with a clean working tree, the closeout error is cosmetic — **STOP retrying finalize** (do NOT `reset --from-current`, which re-poisons the baseline). Just `mv .agent-doc/state/cycles/<dochash>.json` (and any regenerated `op-capture/<dochash>.json`) aside, then `agent-doc preflight` goes `no_changes:true` / already-committed. Root cause of the whole episode was a stale JB plugin ([[feedback_stale_binary_supervisor_causes_live_crdt_edit_bugs]]); see also [[feedback_stuck_response_captured_cycle_mv_state_cycles_json]], [[feedback_forcedisk_crdt_marker_corruption_retry_on_clean]], [[feedback_agent_doc_stale_capture_blocks_commit]].
