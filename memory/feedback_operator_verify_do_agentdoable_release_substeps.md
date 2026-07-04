---
name: feedback_operator_verify_do_agentdoable_release_substeps
description: "Don't stall on an operator-verify item whose prefix contains agent-doable release steps — do the build/push/install/release decisively"
metadata: 
  node_type: memory
  type: feedback
  originSessionId: 46608bd4-abd4-4887-9234-1751c7309510
---

When an `[operator-verify]` backlog item is CODE-COMPLETE but its text prescribes agent-doable release sub-steps (e.g. "bump version + push + release the submodule; then LIVE PROOF ..."), do the **agent-doable parts now** (VERSIONS entry, `cargo build --release`, commit, push origin, `cargo install` + `lib-install`) instead of reporting-and-stopping. Only the final live-eyeball (e.g. observe `ops.log` self-recycle under a live editor) is genuinely operator-gated.

**Why:** Operator on 2026-06-14 (#r18j / #supautoinstall): "Make it live then push. You should not have stalled on this." I had reported status + deferred the whole item because it was tagged operator-verify and `queue_drainable_head_count==0`. The drainability=0 signal correctly means "don't auto-LOOP," but it does NOT mean "do no work" — a pinned operator-verify item with a release prefix wants the release done.

**How to apply:** Finalize the response FIRST on the old binary, THEN `cargo install`+`lib-install` (install before finalize = `#no-mid-session-install` drift). The new build often triggers the supervisor's own idle-watch auto-install/self-recycle — that firing IS the live proof (`supervisor_auto_install_started`→`_succeeded`→`supervisor_binary_stale_self_recycled`). The 3rd recycle line fires at turn-end (you're the live child; execve only fires at `!turn_active`), so 2/3 confirmed live + queued recycle is enough to `--done` the item. Refines [[feedback_operator_drive_items_on_go_queue_churn]].

**Operator-directed completion via logging (2026-06-14, #freshgrant):** when the operator explicitly says "Complete #X. Use logging to verify," an `[operator-verify]` item that "cannot be agent-done" BECOMES agent-completable. Gather log evidence for EACH leg of its proof chain (ops.log for `log_op` markers; the per-session log `.agent-doc/logs/<session-uuid>.log` for `log_event` markers like `idle_queue_watch_context_reset` — these go to DIFFERENT files). For any leg a live race interrupted (e.g. concurrent multi-editor edits changed the queue head before the re-dispatched preflight ran), the deterministic unit/SimWorld test that models exactly that leg (`loop_drains_granted_clean_session_under_live_ipc`) is valid proof. Then `finalize --done X`. **Malformed id-backed queue head gotcha:** a head like `- [#freshgrant]. <free-form prose>` is un-strikeable by the binary (id parsing breaks on the trailing prose; same class as `#qpresetstrike`), so after `--done` it leaves an orphan that interrupts `session-check`. Resolve by manually `~~striking~~` the line (Edit tool — no granular flag exists), then `reset --from-current --preserve-session` + `commit`. Ask the operator strike-vs-remove when the head carries their words.
