---
name: feedback_recycle_deadlock_open_cycle_end_turn_dont_poll
description: "Stale-binary supervisor + open (response_captured) cycle = recycle deadlock; FIXED in agent-doc 0.34.52 (#recycledeadlock). Recovery: end the turn, don't poll."
metadata: 
  node_type: memory
  type: feedback
  originSessionId: ececdc46-67d1-4801-8236-f0f4e57eb992
---

A stale-binary route-owned host supervisor whose finalize wedged (left the cycle in `response_captured` / `write_applied`) will NOT recycle on `agent-doc admin recycle`: the old `supervisor_recycle_action` returned `DeferCycleOpen` for any open cycle (winning over `explicit_admin`/`write_wedged`/`reexec_failed`), so the commit needed the fresh binary while the recycle needed the cycle closed — a deadlock that spun `supervisor_recycle_deferred_cycle_open` ~2/sec forever.

**FIXED in agent-doc 0.34.52 (`#recycledeadlock`, submodule `12b1b1a1`, tag `v0.34.52`, installed 2026-06-26).** Part A: the `cycle_open` defer no longer wins when `stale && (explicit_admin || write_wedged || reexec_failed)` — those prove the binary can't commit, so it falls through to RecycleImmediate/EscalateKillRelaunch. Part B: idle_watch force-abandons the wedged cycle before the execve so the fresh supervisor re-dispatches the queue head (restart the interrupted task). Also removed the "Avoid force/discard recovery for stale-binary refresh" sentence from the stale warning.

**Why:** the deadlock only manifests while the harness turn stays active (`turn_boundary` keys off `prompt_visible && !turn_active`). The recycle (and the OLD binary's `#suprecyclespin` 45s stalled-cycle backstop) only fire at a TRUE turn boundary — i.e. when YOUR turn ends.

**How to apply:** after `admin recycle` + `make install-full` on a wedged stale-binary session, do **NOT** poll `admin inspect`/`pgrep` in a loop — each command keeps the turn active so the boundary is never reached and the supervisor never recycles (this is what made it look permanently stuck). **End the turn.** The old binary self-heals at the boundary: `#suprecyclespin` abandons the stalled (`inflight=0`, untouched >45s) cycle, then `explicit_admin`+`stale` → RecycleImmediate → execve onto the fresh binary, which re-dispatches the interrupted queue head. Do NOT run a `finalize` from the freshly-installed (newer) CLI against the not-yet-recycled OLD supervisor — that is the `live_prompt_drift` newer-client-vs-older-supervisor hazard ([[feedback_no_mid_session_install_against_live_supervisor]]). See also [[feedback_stale_binary_supervisor_causes_live_crdt_edit_bugs]] and [[feedback_stop_cutting_agent_doc_releases]] (bump/commit/install/push/tag done, 3 registry-publish steps skipped).
