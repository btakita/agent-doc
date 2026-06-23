---
agent_doc_session: b26b9957-8de6-4eae-bb98-5d7fe4d6b781
agent: codex
claude_model: opus
agent_doc_format: template
agent_doc_write: crdt
claude_args: --dangerously-skip-permissions
codex_args: -s danger-full-access
pending_done_guard: warn
agent_doc_lint_dialect: off
prompt_presets:
  '#advance-review': Go through the review items. Add actionable backlog items to complete or advance the review items.,
  '#agent-doc-bug': Please create a plan for agent-doc to fix this issue. Add to the backlog of tasks/agent-doc/agent-doc-bugs2.md
  '#code-review': Please review agent-doc codebase. Inspect the architecture and look for edge cases. Isolate concurrency issues and other workflow bugs. Let's find root causes to fix.
  '#commit-push': commit + push all uncommitted files
  '#follow-up-backlog': Any follow-up items to place in the backlog?
  '#mermaid': Include explanatory mermaid diagram(s).
  '#next-steps': Any follow-up items to place in the backlog? Please ensure the backlog is in priority order. Print the item id of the top of the backlog.
  '#simworld': Strengthen Simworld tests.
  '#spec-test-build-install-commit-push': update spec + tests. build + install for local testing. commit + push
  '#spec-test-commit-push': update spec + tests. commit + push
queue: start
---

## Status

<!-- agent:status patch=replace -->

completed
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7a9b103bd5b68223dac05a4c449ac03899aba53d6022e82100578b5b0-20260623-212044.md`*

Compacted content:
- Archived 6 response topic(s): JB Run Agent Doc submit latency; #turnstuck-readybusy; answered queue residue cleanup; 3 more
- Prior summary/context: prior compacted content: Archived 4 response topic(s): actor-switch rebind already covered; paused queue cleanup; #qn...

### Re: ack-mismatch shorter recovered response loss — gpt-5

> **Queue prompt:** do tasks/agent-doc/plan-ack-mismatch-shorter-recovered-response-loss-2026-06-23.md

Implemented `tasks/agent-doc/plan-ack-mismatch-shorter-recovered-response-loss-2026-06-23.md` and continued without waiting on CI per operator instruction.

In `agent-doc-orchestration/src/write/converge.rs`, shorter editor ACKs that are missing only a newly materialized `### Re:` block now replay the missing target response back into the editor and complete as editor IPC convergence. The existing stale extra-prompt artifact path and real user-drift path remain fail-closed.

In `agent-doc-orchestration/src/snapshot.rs`, stale overlay CRDT projection mismatches now rebuild the overlay sidecar from the fallback baseline on first detection. The next merge-base read uses the repaired overlay, so repeated stale-overlay logs and the hot loop stop.

Verification:
- `cargo test -p agent-doc-orchestration pending_write_shorter_ack_replays_missing_agent_response`
- `cargo test -p agent-doc-orchestration crdt_merge_base_state_rebuilds_stale_overlay_after_first_fallback`
- `cargo test -p agent-doc-orchestration ack_mismatch`
- `cargo test -p agent-doc-orchestration crdt_merge_base_state`
- `make check`
- `make install-full`
- `agent-doc --version` -> `agent-doc 0.34.42`
- `git diff --check`

Commits:
- `src/agent-doc`: `08c2c8f3 fix: recover shorter ack mismatches`
- parent: `9069d9fc7 fix: recover agent-doc shorter ack mismatches`

CI was sampled but not awaited: agent-doc CI run `28051475833` was still `in_progress` when checked.

### Re: #directchatpb2 — gpt-5

> **Queue prompt:**
>
❯ > JB `Run Agent Doc` still does not quickly submit.


> **Queue prompt:**
>
> [#directchatpb2]


Implemented `#directchatpb2`.

The fix adds a Codex Stop-hook final gate for active session-document work that is still only a chat answer: if the owned pane has an unresolved exchange prompt or current queue head and the response has not crossed `agent-doc finalize` / `agent-doc write --commit`, the hook blocks final success, captures the replayable answer into the pending/capture ledger, and prints the concrete recovery commands including `agent-doc session-check`.

It also tightens the free-text queue provenance guard: `<!-- no-free-text-queue-head-guard -->` still suppresses intentional removals, but not the malformed `no-free-text...` plus bare `###` interrupted-closeout residue.

Coverage added:
- direct-chat manual queue answer after recursive direct-invocation guard
- consecutive direct-chat answers in the same owned Codex pane
- marker plus bare-heading residue in `session-check`
- preservation of existing clear-command/slash-command queue behavior

Commits:
- `src/agent-doc`: `c2a37292 fix: block direct-chat agent-doc closeout loss`
- parent: `58c7f0076 fix: update agent-doc direct-chat closeout guard`

Verification:
- `cargo test -p agent-doc-orchestration direct_chat`
- `cargo test -p agent-doc-orchestration free_text_queue_marker`
- `cargo test -p agent-doc-orchestration codex_hook::tests::`
- `cargo test -p agent-doc flowcore_hot_path_guard_and_proof_tokens_are_budgeted`
- `make check`
- `git diff --check`
- `gh run list --workflow CI --limit 1` -> latest CI was `completed success` (`28051475833`)

### Re: JB Run Agent Doc submit path — gpt-5

> **Queue prompt:** JB `Run Agent Doc` still does not quickly submit.

> **Queue prompt:** auto-restart session does restart the agent-doc turn.

That narrows the remaining failure: the restart/turn-start path is working, but the post-restart submit path is still late or dropped. I would not treat this as another auto-restart failure; the next useful proof is the route/dispatch window around JB `Run Agent Doc`:

- harness restart or ready prompt observed
- dispatch-ready proof emitted
- submit/send proof emitted or missing
- elapsed time between ready and submit

This keeps `#jbrundispatch` scoped correctly: capture the live stall at submit time and compare ops.log route trace against `route_dispatch_submit_recycle_settle`, `auto_start_dispatch_ready_confirmed`, and any missing dispatch-start or submit proof. If the turn starts but prompt text is not submitted quickly, the fix belongs in the JB route/submit readiness path, not the restart boundary.

### Re: auto-restart narrows JB submit bug — gpt-5

> **Queue prompt:** auto-restart session does restart the agent-doc turn.

Confirmed. I will treat auto-restart as working for this report. The remaining failure is narrower: after the agent-doc turn restarts, JB `Run Agent Doc` still does not submit the prompt quickly enough, or the submit proof is missing.

That keeps the next investigation scoped to the route/submit readiness path, not the restart boundary. The useful live capture is the moment after the restarted pane is ready: whether `auto_start_dispatch_ready_confirmed` appears, whether a dispatch-start/submit proof follows, and how long the gap is before input lands.

### Re: repaired JB submit prompt — gpt-5

> **Queue prompt:**
>
> :pushpin: do [#1jec]: Works except the prompt still does not submit. café 日本 😀日本 😀日本 😀日本 😀日本 😀日本 😀


> **Recovered prompt:** JB `Run Agent Doc` still does not quickly submit.

This is the same remaining JB route issue, now with auto-restart narrowed out. The queue evidence says the agent-doc turn restarts; the unresolved failure is prompt submission after readiness. I am treating the restarted-turn evidence as accepted and leaving the remaining work with the existing live-submit capture items rather than adding a duplicate backlog entry.

### Re: recovered JB submit prompt — gpt-5

> **Queue prompt:**
>
> Turn response is overwriting parts of the previous response in an uncommitted state...so presumably during the finalize step.


> **Recovered prompt:** `> JB Run Agent Doc still does not quickly submit.`

The recovered prompt is preserved and answered as the same JB submit-latency report. Current interpretation: auto-restart does restart the agent-doc turn, so the remaining defect is after readiness, where submit either arrives late or lacks proof. No new backlog item is needed because the existing live-submit capture work already covers that narrowed failure.

### Re: queue items struck without work — gpt-5

> **Queue prompt:** Queue items are being struck without being worked on.

Confirmed. This happened in this recovery: `#1jec` was struck in the queue while the response was only repairing and narrowing the JB submit/restart prompt, not actually live-verifying `#1jec`. That is wrong behavior.

I added `#qstrikework` to the backlog to make queue striking proof-scoped: a queue item should only be struck when the current response proves that exact head was handled, or when an explicit pending/review operation targets its backing id. Interrupted finalize recovery must not auto-quote or strike a later queue head just because it became visible after the earlier head was consumed.

### Re: IPC issue dogfooding log — gpt-5

> **Queue prompt:** Immediately log IPC issues in this document's sessions for dogfooding when I type into the queue.

Captured. I added this as a concrete dogfooding requirement: when an operator types into this document's queue during a live agent-doc turn, editor IPC problems must be surfaced in this same session exchange immediately, with enough detail to debug from the document itself instead of only from `ops.log`.

The expected note should include the issue class, affected component, recovery path, and any relevant ops-log marker. That covers ACK mismatch, IPC timeout, retry-without-disk-write, direct-write fallback, reconnect/stale-buffer decisions, and queue/editor merge anomalies observed while the queue is being edited.

Changed paths: `tasks/agent-doc/agent-doc-bugs2.md`
Commands: `agent_doc_preflight`, `agent_doc_plan`, `agent_doc_read` for queue/exchange/backlog, `agent_doc_finalize`
Verification: session-check pending after finalize
Confidence: high
Escalation: none; implementation remains tracked in the new backlog item.

### Re: `/clear` flooded during active turn — gpt-5

> **Queue prompt:** `/clear` is being incorrectly flooded into the tmux pane during a turn.

Captured as a distinct bug from the intended supervisor clear-and-continue path. `/clear` should only be injected at a proven reset or supervisor handoff boundary, not repeatedly sent into a pane while an agent turn is active.

I added a backlog item for a turn-state flood guard: suppress duplicate `/clear` dispatch while a turn is running, require explicit session-check or supervisor-defer proof before sending it, and add regression coverage for repeated `/clear` text landing in tmux during an active response.

Changed paths: `tasks/agent-doc/agent-doc-bugs2.md`
Commands: `agent_doc_preflight`, `agent_doc_plan`, `agent_doc_read` for queue/backlog, `agent_doc_finalize`
Verification: session-check pending after finalize
Confidence: high
Escalation: none; implementation remains tracked in the new backlog item.

### Re: IPC dogfood incident after clear-flood closeout — gpt-5

> **Queue prompt:**
>
> do [#p6a0]: Broaden to any agent-doc issues detected in this project PCP...dogfood `#agent-doc-bug`


This session hit the exact class of issue that `#mbd4` is meant to make visible in the document.

Issue class: `ipc_proof_insufficient`
Affected component: `queue` / editor writeback
Invariant: `live_prompt_drift_after_preflight`
Recovery path reported: `content_ours_snapshot_next_cycle`
Ops/session marker: `patch_id=0141387b-14e6-43ae-bd99-822d683d4756`, `turn=cycle-1782252992410`

Observed behavior: the clear-flood response was captured, the first finalize failed because the snapshot head and live document head disagreed, and the recovery preflight later committed the captured response. During that recovery, the live buffer also reintroduced several previously answered queue prompts as unstruck items, and preflight logged `postcommit_worktree_check match=false` with `#postcommit-ipc-worktree-corruption`.

A separate backlog item now tracks the stale-live-buffer queue resurrection. I am not marking `#mbd4` done here because the automation that should log this kind of incident automatically is still open; this entry is the manual dogfooding log for the current failure.

Changed paths: `tasks/agent-doc/agent-doc-bugs2.md`
Commands: `agent_doc_session_check`, `agent_doc_preflight`, `agent_doc_read`, `agent_doc_finalize`
Verification: session-check pending after finalize
Confidence: high
Escalation: none; continue through the session-check result, but do not treat the interrupted IPC proof as a successful closeout by itself.
<!-- agent:boundary:df4d746d -->
<!-- /agent:exchange -->

## Queue

<!-- agent:queue priority preset="#spec-test-build-install-commit-push" priority go -->
- ~~do [#p6a0]: Broaden to any agent-doc issues detected in this project PCP...dogfood `#agent-doc-bug`~~
- do [#mbd4]
- JB `Run Agent Doc` still does not quickly submit.
- auto-restart session does restart the agent-doc turn.
- :pushpin: do [#1jec]: Works except the prompt still does not submit. café 日本 😀日本 😀日本 😀日本 😀日本 😀日本 😀
- Turn response is overwriting parts of the previous response in an uncommitted state...so presumably during the finalize step.
- :pushpin: do [#fbwireverify]: "Operator-gated: needs two live editor/CLI panes the driving session cannot host." Please explain.
- :pushpin: do [#qnodemerge4verify]: New plugin installed. How can I verify UTF-8 during merge? When does this happen? If this is too difficult, just mark this as done. 日本 😀日本 😀日本 😀日本 😀
- :pushpin: do [#tb4q]: please check logs
- do [#qcm1verify]: Use the logs to verify.
- do [#qstrikework]
- do [#pzjy]
<!-- /agent:queue -->
<!--
- 🧪 test edit for [#qnodemerge4wire]
---
Please make backlog replacements for all gated review items. We need to complete the plan, not have any more gates after this round. Let me know when I need to install JB plugin updates. Make sure all JB live gate items have adequate logging to prove/disprove the item. Clear the review items. I will test, files bugs, and you will check the logs.
---
`'/clear' is disabled while a task is in progress
---~~~~
A recent monsterrodholders.md session had much churn after I edited the queue while a turn was running. One churn item included deleting a queue item that I added
---
agent-doc reentrant run on equityfundingsource.md had recovery churn.
```
● The recovery is complete and fully consistent. The binary converged the now-drained queue (empty block, queue_active:
  false), no_changes: true, both responses are committed in the session doc, and both code deliverables are on
  origin/main. The auto-loop terminated naturally.

  Let me record the specific recovery path for this failure mode, since it's a recurring dogfooding pattern
```
---
The last run had a code fence stripped away by agent-doc.
#spec-test-build-install-commit-push
---
What are #next-steps to fix bugs?
-->

## Backlog

<!-- agent:backlog priority queue -->
- [ ] [#pzjy] #agent-doc-bug Prevent stale live editor buffers from resurrecting answered queue prompts after IPC-proof recovery: when `live_prompt_drift_after_preflight` / `content_ours_snapshot_next_cycle` recovery commits a captured response, preserve the committed queue strike state or fail closed instead of accepting stale unstruck queue heads from the editor buffer. Add regression coverage for answered free-text and pinned queue items reappearing unstruck after recovery.
- [ ] [#p6a0] #agent-doc-bug Guard `/clear` dispatch against active-turn flooding: `/clear` may be sent only at a proven reset/supervisor handoff boundary, never repeatedly into an active tmux pane while an agent turn is running. Add turn-state/dedupe gating plus regression coverage for repeated `/clear` text being injected during an active response, while preserving the intended focused-cycle supervisor clear-and-continue behavior.
- [ ] [#mbd4] #agent-doc-bug Dogfood IPC issue surfacing in session docs: when an operator types into this document's queue during a live agent-doc turn and editor IPC degrades, times out, ACK mismatches, retries, falls back, or makes a reconnect/stale-buffer decision, append a visible note to the current session exchange immediately with issue class, affected component, recovery path, and ops.log marker so the session document itself captures the IPC bug.
- [ ] [#qstrikework] #agent-doc-bug Prevent queue items from being struck unless the current cycle proves that exact head was worked: the response quotes the exact free-text head, or a --done/--pending-gate/--review-resolve targets the backing id. Add regression coverage for interrupted finalize recovery where a later operator-verify or free-text head is auto-quoted/struck while the response only repairs an earlier prompt.
- [ ] [#qnodemerge4verify] [operator-verify] #agent-doc-bug #qnodemerge4wire Phase 4 live non-ASCII verification (agent-doc submodule 4650054c; base-hash FFI + JB 0.2.173 + VS Code 0.2.29 reporters committed+pushed, make check + JB gradle + tsc/tests green, cdylib lib-installed). REMAINING operator-gated: (1) install the JB plugin zip from disk (build/distributions/agent-doc-jetbrains-0.2.173-signed.zip) + restart IDEA, and install the VS Code .vsix (editors/vscode/agent-doc-0.2.29.vsix) + reload window — the Kotlin/TS reporter changes need a plugin reinstall, the cdylib FFI is already hot-reloaded. (2) In a live editor, type a NON-ASCII edit (e.g. insert/delete around "café 日本 😀") into a session doc while an agent merge runs; confirm ops.log shows editor_op_recorded kind=insert/delete with BYTE offsets (not UTF-16) and that the concurrent merge aligns to the real edit boundary (no character interleaving / mojibake). The base-hash gate + replay==theirs gate make a misaligned op fall back to the diff-guess safely, so a wrong offset cannot corrupt — verification confirms the ops are ACCEPTED (editor_ops_for_base match), not merely safe.
- [ ] [#fbwireverify] [operator-verify] #agent-doc-bug #fbwireverify Live two-pane make tmux-ci acceptance for #fbwire/#fullboundary Phase 2 (agent-doc submodule 85b173fb; committed+pushed, make check green 4956, clippy clean; NOT installed per STOP-cutting-releases). After make install (cargo install --path . + cargo build --release + agent-doc lib-install) + agent-doc admin recycle at a recycle boundary, drive two live panes: (1) a slow-ACK editor drain must never pile IPC inflight > 1 — confirm ops.log shows convergence_gate_defer lines (unmet=inflight_drained) before each dispatch and NO inflight>1; (2) a wedged (no-ACK) editor must trip the bounded CONVERGENCE_GATE_TIMEOUT_MS (30s) → ERROR-level convergence_gate_force_disk_fallback severity=error + a loadable .agent-doc/playback/<hash>/<cycle>.json artifact, and the drain proceeds (never permanently stuck). Operator-gated: needs two live editor/CLI panes the driving session cannot host.
- [ ] [#tb4q] [operator-verify] #qfocsupverify #agent-doc-bug Live-verify #qfocsup supervisor clear-and-continue for [focused-cycle] heads (agent-doc submodule 67829f3f; committed, make check green, NOT installed per STOP-cutting-releases). After make install + agent-doc admin recycle: with a [focused-cycle] head at the queue head and no in-session-drainable work, confirm the supervisor idle-watch force-clears the session (/clear) and re-dispatches the head to a fresh session instead of going idle. Proof: session-check ui_outcome=deferred_for_supervisor_drain on the yielding turn, then ops.log context_reset + go_drain_dispatch + a fresh agent draining the head.
- [ ] [#qcm1verify] [operator-verify] #agent-doc-bug Install + live-verify the #qnodemerge1 component-scoped merge (agent-doc submodule 52458f47). Code shipped+pushed, make check green (4853). Operator-gated remainder (mid-session install causes finalize-client-newer drift; STOP-cutting-releases gate): at a supervisor-recycle boundary run make install (cargo install --path . + cargo build --release + agent-doc lib-install), agent-doc admin recycle, then drive one edit-during-finalize cycle (type into a queue item while the agent writes an exchange response) and confirm ZERO cross-component splice + no console-output-in-queue corruption.
- [ ] [#1jec] [operator-verify] #yzerverify #agent-doc-bug Live-verify the #yzer/#evmhplugin plugin reconnect re-read (agent-doc 0.34.28 / submodule f6b8ae17, plugin 0.2.172). Code shipped+pushed: binary FFI agent_doc_reconnect_buffer_decision (disk==HEAD via ffi_show_head + buffer==prior-commit via ffi_show_prior_blobs) + pure decide_reconnect_buffer + PatchWatcher.registerRoot reconcile-on-connect (applyReconnectReread) + SimWorld coverage; make check green (4832). Operator-gated remainder (install is unsafe mid-session = finalize-client-newer drift): at a supervisor-recycle boundary run make install (cargo install --path . + cargo build --release + agent-doc lib-install), reinstall plugin 0.2.172, agent-doc admin recycle. Then repro: open a session doc in JB, kill/restart the supervisor so the binary commits while the plugin is disconnected, and on reconnect confirm ops.log shows 'reconnect_buffer_decision decision=reread_disk #yzer' and the editor buffer == HEAD with NO File Cache Conflict; a buffer holding genuine unsaved user edits must instead log decision=keep_buffer and be preserved (editor wins).
<!-- /agent:backlog -->

## Review
 
<!-- agent:review -->
- [/] [#ftqpartialdup] #agent-doc-bug Investigate partial free-text queue item duplication in equityfundingsource.md: pasted repro shows a full queue prompt duplicated as a truncated second line ending "... on that". Confirm whether the #hap7/#gf5n structural-node merge fixes cover same-node partial duplication; if not, add a same-node/free-text conflict guard and regression.
- [/] [#recyclerename] #agent-doc-bug Rename the JB + VS Code "Restart Supervisor Process" menu action to "Recycle Supervisor", and ensure a distinct "Restart" command sits alongside it (operator request attached to #s81q, struck when #s81q was gated but NOT implemented — this re-files it honestly). Restart Agent already exists as the harness-restart command; this is specifically renaming the supervisor-process action to "Recycle Supervisor" (matching the admin recycle semantics) + confirming Restart sits alongside. JB: plugin.xml text rename of AgentDoc.RestartSupervisorProcess (action IDs stay stable, only text changes per the convention) + VS Code command title. Confirm alongside-vs-replace with operator. Plan: tasks/agent-doc/plan-restart-agent.md
- [/] [#arrcodexproof] [recommended] #agent-doc-bug #agentreloadrestart Phase 1b follow-up: re-run the managed codex capability proof when a boundary restart switches TO codex. Today spawn_managed_capability_proof_thread runs once before the supervisor loop with the launch harness; build_harness_launch_spec re-resolves args/env/network on restart but capability_proof_required is discarded at the swap, so a mid-session switch to codex spawns fresh but skips the managed capability proof. Wire the proof (re)start into the harness-change restart path. make check + a focused check.
- [/] [#supinstallvis] [recommended] #agent-doc-bug #supinstallfeedback follow-up: the supervisor auto-install status (surface_supervisor_auto_install_status, idle_watch.rs) is surfaced ONLY as a transient tmux display-message on the status line, which is easy to miss while the pane scrollback keeps showing the misleading post-exit "Press Enter to restart" keepalive — so an operator watching the pane body still perceives a ~1-min stall. Residual: make the rebuild state visible in the pane BODY/scrollback (not just the status line) without disturbing the keepalive Enter-to-restart reader, e.g. a pane title set + a single non-input-consuming visible line, or render it through the JB/VSCode plugin notification surface (overlaps #qpauseux notification rendering). Decide the safe pane-write mechanism with the operator before implementing (send-keys risks being consumed by the keepalive). Plan TBD.
- [/] [#runexitrestart] route/idle-watch should detect a cleanly-exited harness pane (Press Enter to restart) with prompt-bearing work, restart the harness, wait for dispatch-ready, then submit, so a cleanly-exited harness never drops a Run Agent Doc. Two-pane live verify. NOTE the originally-reported silent loss was already fixed in 0.34.18.
- [/] [#docdriftgrace] OPERATOR-WATCH one live re-baseline cycle: binary should classify mid/between-turn drift by component and silently auto-rebase snapshot/CRDT for unrelated changes, surfacing only structural conflicts on the in-flight prompt/response. Plan: tasks/agent-doc/plan-graceful-doc-drift-resolution.md
- [/] [#jbrundispatch] OPERATOR-CAPTURE live repro at stall (admin detect for cross-doc pane contamination + ops.log route trace + which pane state failed) to distinguish a real new route/submit bug from already-fixed behavior. Overlaps jbrunstart / startup-miss supersession / qdispatchloss; cannot be fixed blind in route.rs.
- [/] [#cyh0] OPERATOR-CAPTURE: the remaining File Cache Conflict has an unidentified trigger (hot-path disk writes are already listener-guarded). Capture which exact action raises the dialog (agent-doc cmd + ops.log line, e.g. write_authority action=... transport=disk). Likely: ack-content sidecar write, VCS-refresh/save-document signal, or an editor-IPC convergence the plugin surfaces as conflict.
- [/] [#8bfz] single live JB plugin consumer per document (per-doc owner lease; non-owners defer patch apply + saveDocument) so concurrent windows cannot race into File Cache Conflicts. Plugin PatchWatcher + binary lease primitive; operator multi-window live test.
- [/] [#qpauseux] OPERATOR-TEST (plugin render; needs JB plugin build): install updated plugin, verify a paused-queue block renders as an info/action notification with a Resume button, and the churn-stop/no-submit variant surfaces a restart-supervisor/resume action instead of silence (not a raw red Error toast).
- [/] [#jbdisprecycle] OPERATOR-TEST (code R1-R4 shipped submodule d21079b7): at a recycle boundary fire JB Run Agent Doc on a 2nd doc during the hot-reload burst; confirm a single dispatch_start_proof WITH submit and no stranded start_session failed. ops.log marker: route_dispatch_submit_recycle_settle.
- [/] [#editorbufwin] merge-core follow-up after P1 ship (submodule 9f842ba2): #pcwc now preserves raw user queue additions when replay normalization neutralizes the queue component. Remaining P2: auto-recover/commit the preserved queue addition instead of leaving detect-and-warn local drift/manual reset; add regression and live verify stale supervisor CRDT refresh path if still needed. Plan: tasks/agent-doc/plan-editor-buffer-wins-queue-merge.md
- [/] [#s81q] [recommended] #agent-doc-bug #stopvscodemenu Extend #restartagentmenu (JB Restart Agent; plan tasks/agent-doc/plan-restart-agent.md) per operator request to also add a Stop Agent command and mirror BOTH Stop+Restart into the VSCode extension menus. Delta beyond #restartagentmenu: (1) StopAgentAction.kt + plugin.xml registration on the JB side; (2) VSCode command-palette + editor/title menu contributions for stop+restart wired to the same TerminalUtil/route paths. Decide alongside-vs-replace the existing Restart Supervisor Process action; do not remove without operator confirmation. Bump pluginVersion + build signed/unsigned zips (gated by STOP-cutting-releases). Confirm scope before implementing.
- [/] [#restartagentmenu] restart-agent Phase 2 (after agentreloadrestart): RestartAgentAction.kt JB menu cmd mirroring RestartSupervisorProcessAction.kt + plugin.xml registration + TerminalUtil restart wiring; bump pluginVersion; build signed+unsigned. Log action invocation + resolved harness. Decide alongside-vs-replace Restart Supervisor Process with operator. Plan: tasks/agent-doc/plan-restart-agent.md
- [/] [#agentreloadrestart] restart-agent Phase 1: supervisor restart re-resolves harness from current frontmatter (agent claude->opencode takes effect on restart) + boundary auto-restart when resolved harness differs, behind agent_doc_agent_change_restart (default ON). Log harness_change_detected old/new + agent_restart_boundary_gate + agent_restart_performed. SimWorld + make tmux-ci. Plan: tasks/agent-doc/plan-restart-agent.md
- [/] [#j9ja] [/] [blocked] #optverify SUCCESS ops.log markers so operator live tests auto-verify. SHIPPED this cycle: convergence_gate_converged_dispatch (agent-doc submodule 2367d883 — #fbwire Phase 2 success-after-defer in idle_watch.rs, emitted only when the gate had been deferring; auto-verifies #fbwireverify slow-ACK). Already exists: route_dispatch_submit_recycle_settle (route/dispatch.rs:831), auto_start_dispatch_ready_confirmed, queue_paused_continuation_guidance_emitted. BLOCKED (code sites do not exist yet): agent_restart_performed — add inside #agentreloadrestart Phase 1b restart EXECUTION (today only harness_change_detected + agent_restart_boundary_gate note=phase1b_execution_pending exist); restart-agent menu-invoke marker — add inside #restartagentmenu RestartAgentAction. Un-gate + close when those two parent features land and their markers are added.
<!-- /agent:review -->

<!-- agent:icebox -->
<!-- /agent:icebox -->

## Completed / Reaped

<!-- agent:done archive=tasks/agent-doc-bugs2.done.md -->

<!-- completed work archived in tasks/agent-doc-bugs2.done.md -->
<!-- /agent:done -->
