---
name: reference_agent_doc_gate_deadlock_reset_from_current_and_id_bearing_re_block
description: "preflight/write gate deadlock (drift guard vs already-committed gate) escapes via `reset --from-current --preserve-session`; a reaped `do` id with no id-bearing `### Re:` leaves a retained effect that only an id-carrying response block clears"
metadata: 
  node_type: memory
  type: reference
  originSessionId: d7fbfdfd-8635-41e0-95b5-c7284200526f
  modified: 2026-07-25T21:15:53.478Z
---

Two chained wedges seen 2026-07-25 on tasks/professional/job-offers.md (agent-doc 0.35.28), after a controller kill + editor-replica reconnect:

**1. Gate deadlock (binary labels it "committedwedge").** Three commands refuse in a circle:
- `preflight` → `[session-check] INTERRUPTED: cycle X is committed but the document has uncommitted exchange changes beyond the committed snapshot` → says run `finalize`/`write --commit`.
- `write --commit` → `[finalize] pre-write gate: cycle already committed; refusing to replay` → says run `preflight`.
- `agent-doc commit` → "working tree has response-bearing exchange edits" → says run `write --commit`.

The "drift" was normal new-turn state: operator flipped `queue: stop`→`start`, typed a new prompt after the boundary, plus a boundary-hash writeback (and a stray leaked boundary-hash line after `<!-- /agent:done -->`). Escape: verify live buffer == disk (`agent-doc read | tail` vs file tail — both converged, no text at risk), then `agent-doc reset --from-current --preserve-session <FILE>` to re-baseline snapshot/CRDT from the current visible markdown. session-check returns ok immediately after.

**1b. Same escape, different error pair (`typed_component_drift`, seen 2026-07-26 on tasks/software/lazily.md, 0.35.28).** A prior session patched a `### Re:` block straight into the doc and committed it, so:
- `preflight`/`session-check` → `found likely direct response patchback without agent-doc cycle: ### Re: <topic>` … "No response body is pending, so this is document-only drift: run `agent-doc commit`".
- `agent-doc commit` → `refusing to auto-adopt committed historical response patchback: HEAD contains an out-of-band typed_component_drift mutation with response marker ### Re: <topic>`.

Same circle, and `commit` is the command preflight just told you to run. Escape is identical: confirm nothing is at risk (`agent-doc read <FILE>` vs the file — full diff, not just the tail, since the drift can sit mid-document), then `reset --from-current --preserve-session`. After the reset the error DOWNGRADES to the ordinary "snapshot does not match HEAD" drift, and `agent-doc commit` then succeeds — so run reset, commit, session-check in that order and do not conclude the reset failed because the second error text differs from the first.

**2. Retained effect from reaped id without `### Re:` record.** After reaping a queue head with `--done <id>` in a cycle whose response block's header does NOT contain the hash-prefixed id token, ops.log logs `directive_response_materialized id=<id> found=false` + `reaped_queue_head_without_response_fired`, and preflight blocks with `retained document-write effect remains unsettled` even while `session-check` prints ok. The retained intent clears only when a `### Re:` block carrying the literal hash-prefixed id lands. Fix: one `agent-doc write --commit` whose response header is `### Re:` followed by the hash-prefixed id (a short record-repair paragraph pointing at the real answer is enough). Prevention: when closing `do`-directive heads, put the hash-prefixed id token in the `### Re:` header of the answering block.

Caveat: that repair write advances the boundary past ANY pending user prompt in the tail — fold answers to all pending prompts into the same write or they get stranded as "answered".

The terminal `queue consume: queue_active is true but document queue has no prompt to consume` error (exit 1) after the write is harmless — commit had already landed; confirm with `session-check` (per [[reference_agent_doc_stale_plugin_ack_wedge_reload_lib_unwedge]]).

Related: [[reference_agent_doc_recovery_index]], [[reference_agent_doc_recovery_replay_duplicate_boundary_wedges_integrity_gate]] (inline/duplicate boundary marker variant — that one is operator-gated, this one is not).
