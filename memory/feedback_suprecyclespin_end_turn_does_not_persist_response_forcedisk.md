---
name: feedback_suprecyclespin_end_turn_does_not_persist_response_forcedisk
description: Stale-binary supervisor recycle-spin behind zombie controllers does NOT self-heal by ending the turn; response stays uncommitted — recover actively via force-disk.
metadata: 
  node_type: memory
  type: feedback
  originSessionId: ad158c7b-b34c-4fa4-903a-eb5264002e67
---

When `agent-doc finalize` wedges with `invariant=supervisor_binary_stale` / `guard=live_prompt_drift` AND the fresh controllers are stuck `--handoff-state preparing` because their `--previous-controller-pid` predecessors are **defunct/zombie** (`Z<+`), the "end turn, don't poll, it self-heals" advice ([[feedback_recycle_deadlock_open_cycle_end_turn_dont_poll]]) is WRONG. This is **recycle-spin** (`suprecyclespin`): generation counters keep advancing (e.g. gen 2497→2499) but each recycle re-wedges behind a *fresh* set of zombies, so the idle-boundary recycle never drains the retained pending response. Ending the turn left the response **uncommitted** — the operator had to call it out (#agent-doc-bug).

**Why:** the 0.34.52 self-heal assumes one clean recycle fires at the idle boundary. It does not cover a supervisor that recycles repeatedly and re-blocks on zombie predecessors it can't `wait()`-reap. `admin reap-stale-controllers --dry-run` reports "0 would be terminated" because the wedged controllers are still within the stuck-handoff threshold, so that tool won't clear them either.

**How to apply:** don't wait/poll and don't just end the turn. Recover actively with operator-authorized force-disk:
1. `git checkout HEAD -- <FILE>` to drop failed-write boundary residue. NOTE: `agent-doc read` materializes the live-CRDT prompt even when git HEAD never contained it, so checkout can drop the user's prompt from the working tree — re-insert it (direct prompt patching is explicitly allowed before the response exists).
2. Re-insert the missing user prompt right after the HEAD boundary marker, before `<!-- /agent:exchange -->`.
3. `agent-doc reset <FILE> --from-current --preserve-session --force-disk` to sync snapshot to the corrected visible doc.
4. `agent-doc finalize <FILE> --force-disk --origin skill` with the response — it appends after the prompt, repositions the boundary, and commits directly, bypassing the wedged IPC. CRDT merge is `ours-wins`.
5. A trailing `cycle ... is abandoned` error after the commit is a stale-cycle closeout-record failure, NOT a write failure — verify HEAD with `git show HEAD:<FILE> | grep`, then `agent-doc session-check` (it reports `suprecyclespin_stalled_cycle_resolved` = ok).

**Escalation — force-disk itself gets blocked (`visible editor buffer differs from expected disk state`).** After several suprecyclespin cycles the stale supervisor can scramble the CRDT (reordered/duplicated response lines, `prompt_duplication_in_ack_content duplicate_prompt_count=N`) AND diverge the live JetBrains editor buffer from disk. At that point `finalize --force-disk` refuses with `visible editor buffer for <FILE> differs from the expected disk state; save or discard the editor buffer, then retry`, and the editor may flush its scrambled buffer (with `❯`-prefixed prompt markers) onto disk. **Do NOT keep forcing** — repeated force attempts scramble the exchange further and risk dropping operator text. The CLI cannot reconcile a JB in-memory buffer. Correct handoff: (1) confirm the last good `agent-doc(<doc>)` commit is intact in `git log -- <FILE>` and `git show HEAD:<FILE>` is clean (prior work is safe — only the current cycle is unpersisted); (2) deliver the current response to the operator in chat so they have it regardless of doc state; (3) tell the operator to **discard/close the JB editor buffer for that file** (disk becomes authoritative), and if the supervisor binary is stale relative to source, `make install` + `agent-doc admin recycle` (operator's call when it's in-progress code) — then re-invoke and persist cleanly. Stop fighting the editor; it is protecting operator-visible text by design.

See [[feedback_ipc_crdt_response_loss_forcedisk_recovery]] and [[feedback_firstcycle_ipc_dedupe_no_head_prompt_recovery]] for adjacent recoveries.
