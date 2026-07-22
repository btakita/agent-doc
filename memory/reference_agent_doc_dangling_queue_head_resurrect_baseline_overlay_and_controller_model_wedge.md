---
name: reference_agent_doc_dangling_queue_head_resurrect_baseline_overlay_and_controller_model_wedge
description: agent-doc keeps resurrecting an operator-deleted queue head — stale baseline overlay.yrs + wedged controller IPC model; recovery recipe
metadata: 
  node_type: memory
  type: reference
  originSessionId: ebe1a0c4-1f61-4220-8c4d-56888019900a
---

Symptom: operator deletes a queue line (e.g. `- :round_pushpin: [#sy71]`, a dangling in-progress-marked id-backed head with NO backing backlog/done item) in the editor, it keeps coming back. This is a controller wedge, not the projection/drain layer (`queue_drainable_head_count=0` is correct — the orphaned-head filter works).

Non-obvious facts learned during recovery:
- **`agent-doc read` reflects the LIVE controller IPC model**, not the on-disk snapshot. If read shows the stale head while the snapshot/disk are clean, the wedge is the controller's in-memory model.
- **`reset --from-current --preserve-session --force-disk` does NOT rewrite `.agent-doc/baselines/<hash>.overlay.yrs`** (it updates `<hash>.md` baseline + active `.agent-doc/crdt/<hash>.{yrs,nodes.yrs,overlay.yrs}` + snapshot only). A stale sy71-bearing baseline overlay keeps re-seeding the snapshot/model. You must aside `.baselines/<hash>.overlay.yrs` by hand.
- The wedged controller reports `state=ready projection_lag=false freshness=fresh` and `live_editors=0 / keep_editor_authority_no_live_replica` — it ignores `operator_text_authority_v1` editor edits (stale editor lease) and re-pushes its model over the operator's deletion.
- **`admin recycle` only marks "recycle at next idle boundary" and does NOT drop the stale model** while it idle-watches. The decisive move is to **kill the lazy `controller serve --project-root <root>` pid** — it respawns on the next `agent-doc read` and reloads from clean HEAD/snapshot.
- A cold controller respawn can **reseed the head from HEAD** if HEAD still has it — so commit the clean content to HEAD BEFORE the reload.

Recovery recipe (verified 2026-07-13 on tasks/professional/equityfundingsource.md, #sy71):
1. Establish the intended sy71-free content on disk (here: the operator's own editor buffer `7f79…jetbrains-<alive-pid>-…` was authority; its `content` field had the fix already). Confirm `sha256(disk) == editor-buffer hash`.
2. Aside dead-PID live-buffers and the stale `.baselines/<hash>.overlay.yrs`.
3. `agent-doc reset <file> --from-current --preserve-session --force-disk` to rebuild snapshot + active CRDT from disk.
4. Clear `active_queue_heads` in `.agent-doc/state/cycles/<hash>.json`.
5. Commit ONLY that file to HEAD (`git add -- <file>; git commit`) so a reload can't reseed from HEAD.
6. **Kill the `controller serve --project-root <root>` pid** (lazy → respawns clean). `agent-doc read` then reflects clean content.
7. Re-run reset to re-baseline against the new HEAD; verify `read`/disk/HEAD/all sidecars agree and probe `read` several times for no re-clobber.

Product bug to file via the doc's `#agent-doc-bug` preset (do NOT edit agent-doc-bugs2.md from another session — [[feedback_session_document_scope_discipline]]): (a) `reset --from-current` must also rebuild/aside the baseline `overlay.yrs`; (b) a wedged controller with a stale editor lease (`live_editors=0`) must adopt `operator_text_authority_v1` buffer content instead of re-pushing its model; (c) `admin recycle` should be able to force-drop a stale per-doc model without a full controller kill. Related: [[reference_agent_doc_stale_plugin_heartbeat_forcedisk_recovery.md]] [[feedback_wedged_controller_and_jb_ide_spin_on_corrupt_crdt_response_in_head]] [[reference_agent_doc_recovery_index]].
