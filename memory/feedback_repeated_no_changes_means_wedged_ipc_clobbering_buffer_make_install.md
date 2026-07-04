---
name: feedback_repeated_no_changes_means_wedged_ipc_clobbering_buffer_make_install
description: "Repeated no-op agent-doc invocations (no_changes) during active live_prompt_drift = wedged IPC clobbering the live buffer on convergence; stale binary now BLOCKS edits, fix is make install + recycle"
metadata: 
  node_type: memory
  type: feedback
  originSessionId: 877e31a5-adb2-44bc-843e-ff4618b9727c
---

When an agent-doc session is already in per-cycle `live_prompt_drift` (stale binary/cdylib forcing direct-write recovery every finalize), and then the operator invokes `agent-doc <FILE>` two+ times in a row and preflight reports `no_changes: true` with an empty queue **even though the operator is typing new prompts in the JB editor** — the operator's edits are not reaching disk. Preflight's `queue: converged live editor buffer` step is overwriting the unsaved JB buffer with the on-disk version *before* reading it, so each freshly-typed prompt vanishes on the next cycle.

This is the escalation signal: the stale binary has gone from adding recovery *churn* (finalize drifts but direct-write+`reset --from-current` still lands the response) to *blocking* the operator from communicating through the document at all.

**Why:** the direct-write recovery masks the root cause indefinitely — it keeps landing responses so the session looks "fine" — but it does nothing for the stale supervisor's wedged IPC, which keeps clobbering the operator's live buffer.

**How to apply:** don't keep hammering preflight/finalize hoping an edit lands. Two consecutive `no_changes` invocations during known drift ⇒ run `make install` in `src/agent-doc` (rebuilds binary + cdylib; auto-recycle marks controllers/supervisors to recycle at next idle boundary), then `agent-doc admin recycle <FILE>`, then **end the turn** so the supervisor recycles onto the fresh binary at its idle boundary. The *next* invocation runs clean: edits sync, `finalize` stops drifting, no direct-write needed. `agent-doc admin recycle` alone does NOT fix it — recycle promotes onto the currently-*installed* binary, which is itself stale until `make install` rebuilds it. Confirm via preflight: the `stale_install` (source-newer-than-build) warning disappears after install; a remaining "live session supervisor running a STALE binary" warning just means the idle-boundary recycle hasn't fired yet — one more invocation clears it. See [[feedback_no_mid_session_install_against_live_supervisor]], [[feedback_stale_binary_crdt_deadlock_direct_write_git_commit_reset]], [[feedback_verify_recycle_via_admin_inspect_freshness_not_lstart]].
