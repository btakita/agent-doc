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

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7a9b103bd5b68223dac05a4c449ac03899aba53d6022e82100578b5b0-20260623-173806.md`*

Compacted content:
- Archived 4 response topic(s): actor-switch rebind already covered; paused queue cleanup; #qnodemerge4verify / #arsm / #cancelturnverify; 1 more
- Prior summary/context: prior compacted content: Archived 6 response topic(s): lazily-rs session sync; JB Sync Tmux Layout deferral; lazily-r...

### Re: JB Run Agent Doc submit latency — gpt-5

> **Queue prompt:** JB `Run Agent Doc` takes a long time to submit. ~10s

Fixed and shipped.

Root cause: dispatch-only reroutes already treated tmux text+Enter acceptance as sufficient, but the direct-pane path still waited for optional Codex/OpenCode dispatch-start proof when a tracker existed. That made the JetBrains action pay the 10s Codex proof timeout after accepted input, which matches the reported delay.

Changed:
- `agent-doc-orchestration/src/route/dispatch.rs`: added `DirectPaneDispatchOptions`, returned immediately after accepted direct-pane input when proof is optional, and added a regression for the accepted-submit fast path.
- `agent-doc-orchestration/src/route/dispatch_only.rs`: wired dispatch-only delivery to the current proof-required policy for both supervisor IPC and direct-pane submit.
- `agent-doc-orchestration/src/route.rs`: documented that editor dispatch-only returns after accepted input while startup/fresh dispatch still waits for stronger proof.

Verification:
- `cargo test -p agent-doc-orchestration dispatch_only_direct_pane_accepted_submit_skips_optional_proof_wait`
- `cargo test -p agent-doc-orchestration dispatch_only_codex_accepts_enter_delivery_without_start_proof_even_with_visible_hooks`
- `make check` green: clippy, 5020 nextest tests, doctests, and medium SimWorld ignored check.
- `make install` completed; `agent-doc --version` reports `0.34.42`; `agent-doc lib-install` auto-marked 2 controllers to recycle at next idle boundary.
- `gh run list --workflow CI --limit 1` in `src/agent-doc`: new run `28045434769` is in progress after push; previous CI run was green. Superproject has no workflow named `CI`.

Commits pushed:
- `btakita/agent-doc` main: `b3ea06fc` (`fix: avoid optional proof wait for editor dispatch`)
- `btakita/agent-loop` main: `f9d01ee2b` (`chore: update agent-doc submodule`)

Changed paths: `agent-doc-orchestration/src/route.rs`, `agent-doc-orchestration/src/route/dispatch.rs`, `agent-doc-orchestration/src/route/dispatch_only.rs`, plus the superproject `src/agent-doc` pointer. Confidence: high. Escalation: none.

### Re: #turnstuck-readybusy — gpt-5

> **Queue prompt:**
>
> After the restart after the last turn, the prompt was not submitted until ~30s later.日本 😀日本 😀日本 😀日本 😀日本 😀日本 😀日本 😀日本 😀


> **Queue prompt:** After the restart after the last turn, the prompt was not submitted until ~30s later.
> **Completed:** do [#turnstuck-readybusy]

Fixed and pushed. I treated the restart/no-submit queue head as the same ready/busy owned-pane stall: the managed Codex pane could report `alive-busy` / `prompt_ready=false` from a stale queued-draft composer cue after actor, supervisor, and controller state had all returned to ready, so route-owned completion kept the pane alive and delayed the next prompt instead of advancing.

Changed:
- Added ready/busy conflict classification and debounce for route-owned completion and idle queue dispatch.
- Limited automatic reconciliation to the recoverable stale `queued draft in composer` blocker; real active-turn, permission, shell-search, hook-review, help, and clean-exit blockers still stop dispatch.
- Added `agent-doc status` warning and recovery guidance for actor/controller/supervisor ready while the live pane probe still says busy.
- Documented the ready/busy invariant in README, SPEC, session routing, and supervisor docs.

Verification:
- `cargo test -p agent-doc-orchestration ready_busy`
- `cargo test -p agent-doc status_flags_owned_pane_ready_busy_conflict`
- `cargo test -p agent-doc-orchestration route_owned_live_pane_busy`
- `make check` passed: clippy, 5025 tests, doctests, and the medium ignored simulation test.
- `make install-full` installed `agent-doc 0.34.42` and the shared library.
- GitHub Actions CI run `28046746235` passed after push.

Pushed commits:
- `src/agent-doc`: `2aa04685 fix: reconcile ready busy owned panes`
- parent repo: `73ee575f1 chore: update agent-doc submodule`

Note: I left an unrelated unstaged submodule change in `agent-doc-orchestration/src/cycle_state.rs` (`#lzsidecaratomic`) untouched.

### Re: answered queue residue cleanup — gpt-5

> **Queue prompt:**
>
> do [#cancelturnverify]: Verified


Closeout repair: struck the duplicate answered free-text queue row so the already-handled restart/no-submit prompt cannot rerun as stale work. No code changes in this repair commit.

### Re: #c7j5 + #cancelturnverify — gpt-5

> **Queue prompt:** If we merge the sidecar logic into the PCP instead of relying on the disk for IPC, would that help with agent-doc cycle reliability? If yes, let's merge sidecar logic into the PCP. Use lazily-rs new StateMachine, ThreadedStateMachine, and/or AsyncStateMachine for a cycle state machine. See [#c7j5], which was originally created in lazily-rs.md...Except, merge the Sidecar into the PCP. I want to eventually have a master State Machine for an agent document. Note that [#lzsidecaratomic] and [#lzsidecarlock] was done on agent-doc from lazily-rs.md. See lazily-rs.done.md

Yes, with one important constraint: this should merge cycle-state mutation order into the PCP/session actor, but keep the JSON sidecar as the durable crash-recovery log. That gives reliability from one in-process owner and a lazily transition table without making every closeout depend on a live controller socket or losing recovery when the controller is down.

I added the implementation plan at `tasks/agent-doc/plan-cycle-state-pcp-state-machine.md` and rewrote backlog `#c7j5` to point at it. The plan supersedes the older narrow RunContext-only pilot: use a PCP/session-actor cycle-state machine, seed it from the sidecar, serialize accepted `CycleEvent`s through the document actor, and emit the sidecar from that owner job as the durable journal. `ThreadSafeStateMachine` is the right lazily primitive for the controller/session actor; `StateMachine` is only appropriate for single-threaded pure tests or local caches, and `AsyncStateMachine` only matters if the controller state graph moves onto an async context. One prerequisite remains explicit: `agent-doc-orchestration` currently consumes registry `lazily = "0.12.0"`, while the local lazily-rs checkout is what exposes the newer multi-context state-machine APIs, so the dependency source/release has to be resolved in the first implementation slice.

For `#cancelturnverify`, I accepted the operator verification and marked the backlog item done. No code was changed in this cycle; verification was inspection plus the committed plan/backlog update. Plan commit: `00599652e` (`agent-doc: plan cycle state pcp state machine`). Escalation: none. Confidence: high on the architecture direction; implementation still needs the staged gate and test plan in the new plan file.
<!-- agent:boundary:7f44f107 -->
<!-- /agent:exchange -->

## Queue

<!-- agent:queue priority preset="#spec-test-build-install-commit-push" priority go -->
- ~~After the restart after the last turn, the prompt was not submitted until ~30s later.日本 😀日本 😀日本 😀日本 😀日本 😀日本 😀日本 😀日本 😀~~
- ~~do [#cancelturnverify]: Verified~~
- do tasks/agent-doc/plan-ack-mismatch-shorter-recovered-response-loss-2026-06-23.md
- :pushpin: do [#1jec]: Works except the prompt still does not submit. café 日本 😀日本 😀日本 😀日本 😀日本 😀日本 😀
- :pushpin: do [#fbwireverify]: "Operator-gated: needs two live editor/CLI panes the driving session cannot host." Please explain.
- :pushpin: do [#qnodemerge4verify]: New plugin installed. How can I verify UTF-8 during merge? When does this happen? If this is too difficult, just mark this as done. 日本 😀日本 😀日本 😀日本 😀
- :pushpin: do [#tb4q]: please check logs
- do [#qcm1verify]: Use the logs to verify.
- ~~do [#arsm]~~
- do [#directchatpb2]
- ~~do [#turnstuck-readybusy]~~
- ~~After the restart after the last turn, the prompt was not submitted until ~30s later.~~
- ~~After the restart after the last turn, the prompt was not submitted until ~30s later.~~
- :pushpin: do [#qnodemerge4verify]: 日本 😀日本 😀
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
- [ ] [#c7j5] #lzadocsm Merge cycle-state sidecar logic into the PCP/session actor via a lazily state machine. Plan: tasks/agent-doc/plan-cycle-state-pcp-state-machine.md. Direction: PCP/session actor owns in-process mutation order; the sidecar remains the durable crash-recovery log; use ThreadSafeStateMachine once agent-doc consumes a lazily version that exposes it; gate sidecar -> shadow -> dual-write -> controller, then remove only duplicate mutation code, not the durable sidecar.
- [ ] [#directchatpb2] #agent-doc-bug Direct-chat owned-pane response loss recurred on 2026-06-23: Codex answered the MRH "2300 words" lint prompt in chat after recursive direct invocation was refused, then recurred again on the follow-up "Remove the max character count cap" prompt; `tasks/monsterrodholders.md` still lacked a matching `### Re:` response and retained queue residue (`no-free-text-queue-head-guard` + bare heading). Add enforced Codex final/Stop gate + recovery regression so each active session-document prompt, including consecutive prompts in one owned pane, cannot complete without `agent-doc finalize/write --commit`. Plan: tasks/agent-doc/plan-direct-chat-owned-pane-response-loss-2026-06-23.md. DO NOT IMPLEMENT until scheduled.
- [ ] [#qnodemerge4verify] [operator-verify] #agent-doc-bug #qnodemerge4wire Phase 4 live non-ASCII verification (agent-doc submodule 4650054c; base-hash FFI + JB 0.2.173 + VS Code 0.2.29 reporters committed+pushed, make check + JB gradle + tsc/tests green, cdylib lib-installed). REMAINING operator-gated: (1) install the JB plugin zip from disk (build/distributions/agent-doc-jetbrains-0.2.173-signed.zip) + restart IDEA, and install the VS Code .vsix (editors/vscode/agent-doc-0.2.29.vsix) + reload window — the Kotlin/TS reporter changes need a plugin reinstall, the cdylib FFI is already hot-reloaded. (2) In a live editor, type a NON-ASCII edit (e.g. insert/delete around "café 日本 😀") into a session doc while an agent merge runs; confirm ops.log shows editor_op_recorded kind=insert/delete with BYTE offsets (not UTF-16) and that the concurrent merge aligns to the real edit boundary (no character interleaving / mojibake). The base-hash gate + replay==theirs gate make a misaligned op fall back to the diff-guess safely, so a wrong offset cannot corrupt — verification confirms the ops are ACCEPTED (editor_ops_for_base match), not merely safe.
- [ ] [#fbwireverify] [operator-verify] #agent-doc-bug #fbwireverify Live two-pane make tmux-ci acceptance for #fbwire/#fullboundary Phase 2 (agent-doc submodule 85b173fb; committed+pushed, make check green 4956, clippy clean; NOT installed per STOP-cutting-releases). After make install (cargo install --path . + cargo build --release + agent-doc lib-install) + agent-doc admin recycle at a recycle boundary, drive two live panes: (1) a slow-ACK editor drain must never pile IPC inflight > 1 — confirm ops.log shows convergence_gate_defer lines (unmet=inflight_drained) before each dispatch and NO inflight>1; (2) a wedged (no-ACK) editor must trip the bounded CONVERGENCE_GATE_TIMEOUT_MS (30s) → ERROR-level convergence_gate_force_disk_fallback severity=error + a loadable .agent-doc/playback/<hash>/<cycle>.json artifact, and the drain proceeds (never permanently stuck). Operator-gated: needs two live editor/CLI panes the driving session cannot host.
- [ ] [#arsm] [operator-verify] #pcwcwarnverify #agent-doc-bug Live-verify the #pcwcwarn post-commit stale-exchange reconcile (agent-doc submodule 5f3fc07d, superproject db7fc3594). Code shipped+pushed+make-check-green (4947). Runs in the binary commit path + editor IPC, so it goes live once the supervisor adopts the new build (admin recycle done). REMAINING (operator-gated, needs a live editor): drive one edit-during-finalize cycle with a stale > Queue prompt blockquote left inside the exchange in a live JB/VS Code buffer; confirm ops.log shows postcommit_exchange_reconciled reason=stale_editor_exchange AND zero recurring postcommit_worktree_check match=false drift on the next cycle (no manual git checkout HEAD needed).
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
