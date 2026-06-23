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

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7a9b103bd5b68223dac05a4c449ac03899aba53d6022e82100578b5b0-20260623-170826.md`*

Compacted content:
- Archived 6 response topic(s): lazily-rs session sync; JB Sync Tmux Layout deferral; lazily-rs.md crash recovery -- gpt-5; 3 more
- Prior summary/context: prior compacted content: Archived 1 response topic(s): durable recycle checkpointing

### Re: actor-switch rebind already covered — gpt-5

> **Queue prompt:**
>
> :round_pushpin: JB `Run Agent Doc` on agent-loop.md after switching from claude to codex. The session switched from claude to codex, but the actor record did not switch.
> ```
> [route] target tmux session: 0
> Error: authoritative actor record for tasks/software/agent-loop.md is bound to harness claude-code, not codex
> ```

This exact failure is already fixed in the installed build. It is the `#actor-switch-rebind` entry shipped in `v0.34.40`: when a document explicitly changes `agent:` from `claude` to `codex`, route now treats the old healthy `claude-code` authoritative actor as stale, logs `route_authoritative_actor_harness_mismatch_stale`, and falls back to the normal create/rebind path for the newly resolved harness instead of hard-failing with `bound to harness claude-code, not codex`.

The current local binary is `agent-doc 0.34.42`, so it includes that fix. The route decision is in `route/authoritative_actor.rs`: a mismatched actor can be replaced when the document frontmatter declares the expected harness, when supervisor health is not healthy, or when the actor is closed. The exact queue shape is also covered by queue parsing/continuation regressions so prose plus fenced route diagnostics drains as real work instead of being pruned as log noise.

I did not add another backlog item because this report is covered by the shipped `#actor-switch-rebind` behavior. A recurrence on `0.34.42` or newer would be a fresh live regression, and the proof to capture would be an ops line missing `route_authoritative_actor_harness_mismatch_stale` during an explicit `agent: claude` -> `agent: codex` frontmatter switch.

Proof:
- changed_paths: this session document via `agent-doc finalize` only
- commands: `agent-doc preflight`, `agent-doc plan`, `agent-doc session-check`, `agent-doc repair --apply-recovery`, `agent-doc patch`, `git diff -- tasks/agent-doc/agent-doc-bugs2.md`, `git show HEAD:tasks/agent-doc/agent-doc-bugs2.md`, `rg` over route/version/queue text, `sed` on `VERSIONS.md`, `route/authoritative_actor.rs`, `queue.rs`, `queue_continuation.rs`, and `write/queue_consume.rs`, `agent-doc --version`, `gh run list --workflow CI --limit 1`
- verification: no code/spec changed, so I did not run `make check`; installed version is `agent-doc 0.34.42`, and the latest `src/agent-doc` CI run for `v0.34.42` is `completed/success`
- escalation: none
- confidence: high

### Re: paused queue cleanup — gpt-5

I picked up the queue-only edit. The struck residue that was removed from the queue stayed removed, and preflight then re-converged the managed queue from the backlog mirror.

I did not start any backlog implementation from this turn. The binary reports `queue_paused=true`, `queue_continuation_required=false`, and `queue_drainable_head_count=0`; the current queue is halted by the existing phantom-pin churn guard, so this cycle is a closeout/acknowledgement rather than an auto-drain. The active backlog still contains operator-verify items, and `#directchatpb2` remains explicitly marked `DO NOT IMPLEMENT until scheduled` in the backlog text.

Preflight also surfaced semantic-completion candidates worth checking before future scheduling: `#1jec` against `#yzer`, `#arsm` against `#pcwcwarn`, `#qcm1verify` against `#qnodemerge1`, review `#ftqpartialdup` against `#hap7`, and review `#j9ja` against `#optverify`.

Proof:
- changed_paths: this session document via `agent-doc finalize` only
- commands: `tsift status`, `tsift status --fix`, `agent-doc --version`, `agent-doc preflight`, `agent-doc plan`, `agent-doc read`, `agent-doc finalize`
- verification: no code/spec changes; no test suite was needed for this prompt-response cycle
- escalation: none
- confidence: high

### Re: #qnodemerge4verify / #arsm / #cancelturnverify — gpt-5

> **Queue prompt:**
>
> :pushpin: do [#1jec]: Works except the prompt still does not submit.


For `#qnodemerge4verify`: no, I would not count the `日本 😀` edit as a full verification pass yet. The good part is that ops.log shows the JetBrains reporter did record `#qnodemerge4wire` editor ops for `agent-doc-bugs2` with byte offsets and base hashes, including the current-cycle inserts at offsets `5207` and `5218` with base `363b3b483356`. That proves the non-ASCII typing path is emitting byte-indexed ops.

The missing part is the acceptance proof. The same log window reports `crdt_merge_base source=fallback_overlay_projection_mismatch`, and I did not find an `editor_ops_for_base` acceptance marker for this document/cycle. Since the backlog item explicitly requires accepted base-matched ops, not just safe fallback behavior, `#qnodemerge4verify` stays open.

For `#arsm`: agreed, this is not verified. The recent log only shows `go_queue_mirror_deferred id=#arsm reason=operator_verify`, and I do not see the required `postcommit_exchange_reconciled reason=stale_editor_exchange` plus a clean next-cycle `postcommit_worktree_check match=true` proof for this prompt. The visible symptom you reported, a free-text queue item left open, means the item remains failed/open rather than done.

For the earlier `#cancelturnverify` prompt: I found and fixed the JetBrains menu issue. `Cancel Turn` was registered in `ToolsMenu`, `EditorPopupMenu`, and `ProjectViewPopupMenu`, but it was missing from the custom Agent Doc popup list. I added `AgentDoc.CancelTurn` to `AgentDocPopupAction.PRIMARY_ACTION_IDS`, added test coverage, bumped the JB plugin to `0.2.178`, built the signed and unsigned zips, committed, and pushed both the submodule commit and the superproject pointer. The remaining `#cancelturnverify` step is only operator install/live verification.

Proof:
- changed_paths: `src/agent-doc/editors/jetbrains/gradle.properties`, `src/agent-doc/editors/jetbrains/src/main/kotlin/com/github/btakita/agentdoc/AgentDocPopupAction.kt`, `src/agent-doc/editors/jetbrains/src/test/kotlin/com/github/btakita/agentdoc/AgentDocPopupActionTest.kt`, superproject `src/agent-doc` pointer, and this session document via `agent-doc finalize`
- commits: `src/agent-doc` `e73ca1a8` pushed to `btakita/agent-doc`; superproject pointer commit `22766ff40` was pushed, with later agent-doc session commits now ahead in the root history
- commands: `tsift status`, `agent-doc preflight`, `agent-doc plan`, `agent-doc read`, `rg`, `sed`, `./gradlew test --tests com.github.btakita.agentdoc.AgentDocPopupActionTest`, `make check`, `make bump-plugin`, `git add`, `git commit`, `git push origin main`, `gh run list --workflow CI --limit 1`, targeted `rg ... ops.log | tail`, `agent-doc session-check`, `agent-doc finalize`
- verification: focused JetBrains popup test passed; `make check` passed, including 5019 nextest tests plus the medium SimWorld check; `make bump-plugin` built `agent-doc-jetbrains-0.2.178.zip` and `agent-doc-jetbrains-0.2.178-signed.zip`; latest `btakita/agent-doc` CI run `28043989387` was `in_progress` after the push; root `btakita/agent-loop` reported no workflow named `CI`
- escalation: none
- confidence: high

### Re: closeout guard repair — gpt-5

I repaired the closeout bookkeeping from the previous response: `#qnodemerge4verify` and `#arsm` are explicitly narrowed to their remaining proof, `#cancelturnverify` records the new JB plugin build, and the unhandled `#1jec` / `#fbwireverify` queue prompts are restored instead of silently struck or flattened.
<!-- agent:boundary:88822d3c -->
<!-- /agent:exchange -->

## Queue

<!-- agent:queue priority preset="#spec-test-build-install-commit-push" priority go -->
- :pushpin: do [#1jec]: Works except the prompt still does not submit. café 日本 😀日本 😀日本 😀日本 😀日本 😀日本 😀
- :pushpin: do [#fbwireverify]: "Operator-gated: needs two live editor/CLI panes the driving session cannot host." Please explain.
- ~~:pushpin: do [#qnodemerge4verify]~~
- :pushpin: do [#tb4q]: please check logs
- ~~do [#cancelturnverify]: I don't see Cancel Turn in the JB Plugin menu.~~
- do [#qcm1verify]: Use the logs to verify.
- ~~do [#arsm]~~
- do [#directchatpb2]
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
- [ ] [#directchatpb2] #agent-doc-bug Direct-chat owned-pane response loss recurred on 2026-06-23: Codex answered the MRH "2300 words" lint prompt in chat after recursive direct invocation was refused, then recurred again on the follow-up "Remove the max character count cap" prompt; `tasks/monsterrodholders.md` still lacked a matching `### Re:` response and retained queue residue (`no-free-text-queue-head-guard` + bare heading). Add enforced Codex final/Stop gate + recovery regression so each active session-document prompt, including consecutive prompts in one owned pane, cannot complete without `agent-doc finalize/write --commit`. Plan: tasks/agent-doc/plan-direct-chat-owned-pane-response-loss-2026-06-23.md. DO NOT IMPLEMENT until scheduled.
- [ ] [#cancelturnverify] [operator-verify] #agent-doc-bug Live-verify Cancel Turn (agent-doc submodule b4471d05; committed+pushed, make check green, JB 0.2.176 + VS Code 0.2.31 built, NOT installed). After installing the plugins: (1) while a turn is RUNNING, invoke Cancel Turn (JB menu / VS Code command) and confirm the turn is interrupted (ops.log cancel_turn_performed action=interrupt_active_turn) and the agent stays open; (2) while the agent is IDLE, invoke Cancel Turn (even repeatedly) and confirm it is a NO-OP (ops.log cancel_turn_noop reason=idle_no_active_turn) and the agent is NOT closed — the core safety requirement. Verify per harness: Codex/OpenCode (Escape/C-c, never raw Ctrl+C that would close on idle) and Claude.
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
