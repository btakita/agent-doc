---
name: feedback_operator_edited_response_live_buffer_reset_fromcurrent_commit
description: "Operator edits the response in the live JB editor before commit → live_prompt_drift wedge → recover via reset --from-current + agent-doc commit, NOT force-disk/re-inject."
metadata: 
  node_type: memory
  type: feedback
  originSessionId: 2d1f931f-b101-4fda-8c25-3f2a4c0d70a3
---

When the operator edits the assistant response in the live JetBrains buffer *before* it is committed, `finalize`/`write --commit` wedge on `invariant=live_prompt_drift_after_preflight recovery=visible_repair_required`, `source=socket_ack_content`, "editor IPC did not prove the write — refusing direct document write."

**Why:** the response already converged into the live JB buffer via IPC (the "auto-recovered ... convergence (N bytes)" line). Each retry re-appends the candidate on top of the response already in the buffer → would double it, so the tool fails closed to protect operator-visible text. The wedge is mechanical (double-append protection), not loss.

**How to apply:**
1. Do NOT retry finalize/write (re-appends → double) and do NOT `--force-disk` the stdin candidate (discards operator edits — my un-edited version, not theirs).
2. Find the live buffer: `.agent-doc/live-buffer/<hash>.jetbrains-<pid>-...` (JSON; `jq '{len,hash,edit_epoch,last_synced_epoch,no_unsaved_operator_edits}'`). It carries `operator_text_authority_v1` and the full converged content. `content_ours` in the error == this buffer.
3. Wait for it to settle: `no_unsaved_operator_edits: true` and a stable `hash` across two reads (edit_epoch may advance without content change). Back up disk + `jq -j '.content'` to scratchpad first.
4. The editor flushes the settled buffer to disk on its own (disk grows to match buffer len). Then `agent-doc reset --from-current --preserve-session <FILE>` rebuilds snapshot+CRDT from the current visible markdown.
5. `agent-doc commit <FILE>` → HEAD gets the response; `session-check` → `committed (commit_success)`, `ui_outcome=no_drainable_work`.
6. Residual working-tree diff after commit (boundary reposition + `❯` display-prefix stripping on already-answered prompts) is cosmetic: the log says `realtime replica owns reconciliation, no revert (#realtimecutover)` — leave it, the editor catches up. Don't fight it.

ack-content sidecar (`.agent-doc/ack-content/<patch_id>.md`) = the tool's record of what the editor ACK'd its buffer contains after a patch; used as `content_ours` for drift detection and as loss-proof recovery so operator edits are never clobbered.

**ROOT CAUSE FIXED IN-PRODUCT (agent-doc commit 8af0d850, Phase 1, installed 2026-07-02).** The wedge was: proof surfaces required the agent's EXACT response bytes / a byte-exact live-buffer match, so an operator body-edit read as "response missing" and the ack read as `stale_source_buffer` when the operator kept editing past the ack capture. Fixed in the realtime model (`write_policy.rs`: `response_converged_in_visible_target`, `buffer_presents_reference_response`) + `reconcile_ack_snapshot_to_newer_operator_buffer` (adopts the operator's newer live buffer forward). After the fixed binary is installed + supervisor recycled, an operator-edited-before-commit response should reconcile and commit with NO wedge — so this manual recovery is a fallback for stale binaries only. Phase 2-3 (full reconcile-before-accept loop on the per-cell model) tracked in `src/agent-doc/tasks/agent-doc/plan-realtime-reconcile-replicas.md`.

Contrast with [[feedback_send_failed_no_ide_running_forcedisk_not_restart]] (no IDE → force-disk is right). Here a live JB editor with authoritative edits exists, so force-disk is WRONG. Related: [[feedback_finalize_from_wrong_pane_binding_during_fleet_recycle]], [[feedback_agent_doc_document_is_source_of_truth]].
