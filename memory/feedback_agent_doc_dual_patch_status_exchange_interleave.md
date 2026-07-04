---
name: feedback_agent_doc_dual_patch_status_exchange_interleave
description: Combining patch:status + patch:exchange in one CRDT finalize can interleave/scramble the exchange; prefer single-patch finalizes
metadata: 
  node_type: memory
  type: feedback
  originSessionId: bba16ca4-20e1-4db8-9d5f-77bc799961e9
---

A single `agent-doc finalize` carrying BOTH a `<!-- patch:status patch=replace -->` block AND a `<!-- patch:exchange -->` block (CRDT write path, `agent_doc_write: crdt`) is far more prone to the scaffold-interleave corruption than a single-patch finalize. Live repro 2026-06-09 `tasks/professional/equityfundingsource.md` deploy cycle: the status patch applied cleanly but the exchange patch's bullets were spliced (reverse-order) into the PRIOR response, the new `### Re:` header was not created, boundary markers drifted, and finalize failed exit 1 `mixed duplicate scaffold tail ... refusing automatic closeout repair`. The two prior single-`patch:exchange` finalizes that same drain hit only the benign self-recovering exit 75 and committed fine.

**Why:** under the IPC `already_applied` / stale-live-buffer race, the CRDT merge reconciles two components against a buffer that already holds partial content, and the exchange component loses its anchor.

**How to apply:**
- Default to ONE patch component per finalize. Persist the response via `patch:exchange` first; update `agent:status` in a SEPARATE follow-up `agent-doc write --commit` (or its own cycle).
- If a dual-patch finalize fails this way, do NOT keep re-running finalize. The route-owned supervisor's repeated CRDT merges often CONVERGE the document to the correct state on their own within a few passes (HEAD advances each pass). VERIFY before re-editing: `git show HEAD:FILE` for the expected `### Re:` sections + drained queue, `git status` clean, then `agent-doc preflight` until `no_changes:true` / `recovered:false`.
- Beware: running `agent-doc preflight`/`session-check` yourself each starts a cycle (`preflight_diff_start`); a bare `session-check` right after can report `INTERRUPTED` for that dangling start even though content is fine — settle with one `preflight` and read `no_changes:true`.
- Recovery sidecars that re-inject stale content live in `.agent-doc/crdt/<pathhash>.{yrs,overlay.yrs}`, `.agent-doc/state/cycles/<pathhash>.json`, and `.agent-doc/captures/<pathhash>/`. `reset --from-current --preserve-session` KEEPS capture/cycle history so preflight replays it — drop `--preserve-session` (or move those sidecars aside) to stop the replay. Relates to [[feedback_ipc_crdt_response_loss_forcedisk_recovery]], [[feedback_agent_doc_stale_capture_blocks_commit]], [[feedback_agent_doc_supervisor_self_race_finalize_block]].
