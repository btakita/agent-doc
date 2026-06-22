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

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7a9b103bd5b68223dac05a4c449ac03899aba53d6022e82100578b5b0-20260622-082847.md`*

Compacted content:
- Archived 12 response topic(s): Validate/invalidate operator-verify items from the logs; do [#fbwire]; do [#j9ja]; 9 more
- Prior summary/context: prior compacted content: Archived 6 response topic(s): #opslogtrack verified live; make the transport healthy + advan...

### Re: do [#pendaddorder] — gpt-5

> **Queue prompt:** :pushpin: [#pendaddorder]

Closed `#pendaddorder`. The product contract is now explicit: repeated `--pending-add` flags preserve flag order top-down in the backlog, and active `queue: start` same-cycle queue mirroring appends those new ids after the existing live queue tail in the same order.

Changes shipped in agent-doc `6887fe26` and superproject `6891417b7`: clarified the `add_many` order invariant, updated CLI help and `SKILL.md` wording, and added `finalize_pending_add_multiple_flags_keep_cli_order_in_active_go_queue` to cover backlog order plus queue-mirror parity for multiple same-cycle pending adds. `runbooks/pending-ops.md` already had the desired wording.

Verified: focused pending-add order tests, `make check` green (4973 nextest tests plus doctests and the medium SimWorld test), and GitHub Actions CI for `6887fe26` completed successfully. Installed from a clean self-install worktree at the pushed commit; `lib-install` marked 2 controllers for recycle, and explicit `agent-doc admin recycle` for this project reported no running controller.

### Re: Clear Session Context delayed Run Agent Doc dispatch — gpt-5

> **Queue prompt:** :pushpin: JS `Clear Session Context` then JB `Run Agent Doc` does nothing. Subsequent JB `Run Agent Doc` eventually dispatches...it seems there's a delay before working. Please enable JB `Run Agent Doc` to dispatch immediately.

Closed as covered by the shipped clear-cooldown auto-trigger fix. In agent-doc `3d110b3a` / superproject `b1169577d`, startup auto-trigger no longer treats an active clear-cooldown marker as a terminal skip; it keeps polling under the existing 30s startup timeout and only fails with `auto_trigger_timeout reason=clear_cooldown_after_30s` if the cooldown never clears.

That is the behavior this queue item asks for: after Clear Session Context, an explicit JB `Run Agent Doc` should dispatch as soon as the clear cooldown clears instead of waiting for a later/manual attempt to re-trigger it. The fix was already verified with `cargo test -p agent-doc-orchestration auto_trigger_clear_cooldown_waits_until_timeout_instead_of_terminal_skip`, `make check`, local clean-worktree install, and the later full `make check`/CI pass from the `#pendaddorder` cycle.

### Re: Clear Session Context dispatch storm / no-submit report — gpt-5

> **Queue prompt:** :pushpin: JS `Clear Session Context` then JB `Run Agent Doc` sometimes has the dispatch storm (multiple duplicate prompts) and no submit.

Triaged this as the existing route/dispatch storm cluster, with one piece already fixed. The no-submit after clear-cooldown path is covered by agent-doc `3d110b3a`: explicit startup auto-trigger now waits through clear cooldown instead of treating it as a terminal skip.

The duplicate-prompt/storm part is not a new standalone item here; it remains covered by the existing route/dispatch capture and recycle-settle work already in the queue/backlog (`#jbrundispatch`, `#jbdisprecycle`, and related no-submit/retry items). No additional backlog item is needed for this pasted report because it does not add a distinct repro beyond that tracked cluster.

### Re: Frontmatter agent revert / do [#editorbufwin] — gpt-5

> **Queue prompt:** I switched equityfundingsource frontmatter agent from claude to codex, but agent-doc keeps on reverting the frontmatter agent to claude.
> **Queue prompt:** :pushpin: do [#editorbufwin]

I captured the frontmatter-agent revert as `#agentfrontpersist` instead of folding it into `#arrkillguard`: the existing item covers not killing/replacing an in-flight pane when `agent:` changes, while this report says the document’s requested `agent:` value itself is being reverted.

For `#editorbufwin`, I shipped the P1 preservation fix and narrowed the backlog item to the remaining P2 recovery work. The bug was that post-commit worktree reconciliation proved content loss with replay-normalized documents, but that same normalizer neutralizes the `agent:queue` component. A user-added queue line could therefore be invisible to the added-work detector and get clobbered by HEAD.

Implemented in agent-doc `9f842ba2` and superproject `3aaf90672`: `#pcwc` still proves committed-content loss against replay-normalized text, but now detects newly-added carry-forward work from the raw document; `line_is_carry_forward_signal` also recognizes markdown queue/backlog mirrors like `- :pushpin: do [#id]` and `- [ ] [#id] ...`.

Verification: focused tests passed for the carry-forward classifier and pinned-queue post-commit preservation; full `make check` passed with 4975 tests plus doctests and the medium SimWorld gate; GitHub Actions CI passed for run `27941181073`. Installed the pushed build via `agent-doc self-install`; explicit recycle found no running controller for this repo.

### Re: Queue mirror repair for frontmatter prompt / #editorbufwin — gpt-5

> **Queue prompt:** I switched equityfundingsource frontmatter agent from claude to codex, but agent-doc keeps on reverting the frontmatter agent to claude.

The frontmatter-agent report is now captured as `#agentfrontpersist`; this follow-up closes the still-present queue mirror for that prompt after the previous response added the backlog item.

I also gated the remaining `#editorbufwin` work instead of leaving a struck queue mirror with an open backlog item. P1 is shipped in `9f842ba2`; the remaining P2 auto-recover/commit behavior is still tracked under `#editorbufwin` and should run as a focused follow-up rather than being silently removed from the active queue.

### Re: do [#jbdisprecycle] — gpt-5

> **Queue prompt:** :pushpin: do [#jbdisprecycle]

I gated `#jbdisprecycle` rather than marking it done. The item is an `OPERATOR-TEST`: it requires firing JetBrains `Run Agent Doc` on a second document during a hot-reload/recycle burst and confirming the live `route_dispatch_submit_recycle_settle` marker. I cannot honestly perform that IDE/operator action from this CLI-only turn.

The shipped code remains tracked on the item; the remaining proof is the live JB recycle-boundary check.

### Re: do [#sqedit-race] — gpt-5

> **Queue prompt:** :pushpin: do [#sqedit-race]: Validate if the fix is successful by looking at the logs. No `queue_stale_noise_lines` means success.

Validated the current doc state for `#sqedit-race`. `agent-doc queue prune-noise /home/brian/work/btakita/agent-loop/tasks/agent-doc/agent-doc-bugs2.md` reported: no non-drainable noise queue heads to prune. The follow-up `agent-doc session-check` stayed ok and did not emit `queue_stale_noise_lines`.

Marked `#sqedit-race` done. The only remaining warning in the check is the stale route-owned supervisor pid `2283524`, which is separate from the queue-noise predicate.

### Re: do [#qpauseux] — gpt-5

> **Queue prompt:** :pushpin: do [#qpauseux]

I gated `#qpauseux`. It is an `OPERATOR-TEST` for JetBrains plugin rendering: install the updated plugin and visually confirm paused-queue and churn-stop notifications render with the expected action affordances. That requires a live IDE/plugin install and visual/operator confirmation, so I cannot complete it from this CLI turn.

The remaining work is the plugin-render verification itself, not a code change in this pass.

### Re: do [#8bfz] — gpt-5

> **Queue prompt:** :pushpin: do [#8bfz]

I gated `#8bfz`. The remaining work is an operator multi-window JetBrains plugin live test for the per-document owner lease: one plugin consumer should own patch apply/saveDocument for the document and non-owners should defer, avoiding File Cache Conflicts.

I cannot perform that multi-window IDE verification from this CLI-only turn. The code primitive is already referenced by related shipped work; this item remains the live operator proof.

### Re: do [#cyh0] — gpt-5

> **Queue prompt:** :pushpin: do [#cyh0]

I gated `#cyh0`. The remaining task is `OPERATOR-CAPTURE`: reproduce a live File Cache Conflict and capture the exact triggering action plus the adjacent ops.log marker. That requires the operator’s IDE/dialog context and cannot be completed from this CLI turn.

The queue item remains as a gated capture task for the next live FCC occurrence.
<!-- agent:boundary:e2ce5bcc -->
<!-- /agent:exchange -->

## Queue

<!-- agent:queue priority preset="#spec-test-build-install-commit-push" priority go -->
- :pushpin: do [#qcm1verify]
- :pushpin: do [#arsm]
- ~~:pushpin: do [#editorbufwin]~~
- ~~:pushpin: do [#jbdisprecycle]~~
- ~~:pushpin: do [#sqedit-race]: Validate if the fix is successful by looking at the logs. No `queue_stale_noise_lines` means success.~~
- ~~:pushpin: do [#qpauseux]~~
- ~~:pushpin: do [#8bfz]~~
- ~~:pushpin: do [#cyh0]~~
- :pushpin: do [#1vhn]
- :pushpin: do [#jbrundispatch]
- :pushpin: do [#docdriftgrace]
- :pushpin: do [#runexitrestart]
- :pushpin: do [#gf5n]
- :pushpin: do [#supinstallvis]
- :pushpin: do [#1jec]
- do [#fbwireverify]
- do [#qnodemerge4verify]
- do [#arrkillguard]
- do [#arrcodexproof]
- do [#tb4q]
- do [#cancelturnverify]
- do [#recyclerename]
- do [#ftqpartialdup]
<!-- /agent:queue -->
<!--
- 🧪 test edit for [#qnodemerge4wire]
---
Please make backlog replacements for all gated review items. We need to complete the plan, not have any more gates after this round. Let me know when I need to install JB plugin updates. Make sure all JB live gate items have adequate logging to prove/disprove the item. Clear the review items. I will test, files bugs, and you will check the logs.
---
`'/clear' is disabled while a task is in progress
---~~~~
A recent monsterrodholders.md session had much churn after I edited the queue while a turn was running. One churn item included deleting a queue item that I added.
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
- [ ] [#agentfrontpersist] #agent-doc-bug Preserve user-edited session frontmatter `agent:` on equityfundingsource.md: when the operator changes `agent: claude` to `agent: codex`, agent-doc must not rewrite it back to `claude`; distinguish explicit frontmatter edits from route/harness reconciliation. Related #arrkillguard/#agentreloadrestart; needs repro/log capture and regression.
- [ ] [#ftqpartialdup] #agent-doc-bug Investigate partial free-text queue item duplication in equityfundingsource.md: pasted repro shows a full queue prompt duplicated as a truncated second line ending "... on that". Confirm whether the #hap7/#gf5n structural-node merge fixes cover same-node partial duplication; if not, add a same-node/free-text conflict guard and regression.
- [ ] [#cancelturnverify] [operator-verify] #agent-doc-bug Live-verify Cancel Turn (agent-doc submodule b4471d05; committed+pushed, make check green, JB 0.2.176 + VS Code 0.2.31 built, NOT installed). After installing the plugins: (1) while a turn is RUNNING, invoke Cancel Turn (JB menu / VS Code command) and confirm the turn is interrupted (ops.log cancel_turn_performed action=interrupt_active_turn) and the agent stays open; (2) while the agent is IDLE, invoke Cancel Turn (even repeatedly) and confirm it is a NO-OP (ops.log cancel_turn_noop reason=idle_no_active_turn) and the agent is NOT closed — the core safety requirement. Verify per harness: Codex/OpenCode (Escape/C-c, never raw Ctrl+C that would close on idle) and Claude.
- [ ] [#recyclerename] #agent-doc-bug Rename the JB + VS Code "Restart Supervisor Process" menu action to "Recycle Supervisor", and ensure a distinct "Restart" command sits alongside it (operator request attached to #s81q, struck when #s81q was gated but NOT implemented — this re-files it honestly). Restart Agent already exists as the harness-restart command; this is specifically renaming the supervisor-process action to "Recycle Supervisor" (matching the admin recycle semantics) + confirming Restart sits alongside. JB: plugin.xml text rename of AgentDoc.RestartSupervisorProcess (action IDs stay stable, only text changes per the convention) + VS Code command title. Confirm alongside-vs-replace with operator. Plan: tasks/agent-doc/plan-restart-agent.md
- [ ] [#arrkillguard] #agent-doc-bug #agentreloadrestart Phase 1 kill-guard (the real-harm item): route/sync/start must NOT cold-start a replacement pane / evict an in-flight turn solely because frontmatter agent: changed — defer to the graceful boundary restart (#agentreloadrestart Phase 1b, shipped 603ce7a2). Root harm 2026-06-18: agent: claude->opencode on equityfundingsource.md killed the live claude turn (route migrated [claude] %83 -> [opencode] %90 mid-turn). Add a guard in route.rs/sync.rs that refuses to replace a BUSY same-session pane when the only difference is agent:; verify with make tmux-ci across two panes (one mid-turn) that the switch waits for the boundary and never kills the running turn. Plan: tasks/agent-doc/plan-restart-agent.md
- [ ] [#arrcodexproof] [recommended] #agent-doc-bug #agentreloadrestart Phase 1b follow-up: re-run the managed codex capability proof when a boundary restart switches TO codex. Today spawn_managed_capability_proof_thread runs once before the supervisor loop with the launch harness; build_harness_launch_spec re-resolves args/env/network on restart but capability_proof_required is discarded at the swap, so a mid-session switch to codex spawns fresh but skips the managed capability proof. Wire the proof (re)start into the harness-change restart path. make check + a focused check.
- [ ] [#qnodemerge4verify] [operator-verify] #agent-doc-bug #qnodemerge4wire Phase 4 live non-ASCII verification (agent-doc submodule 4650054c; base-hash FFI + JB 0.2.173 + VS Code 0.2.29 reporters committed+pushed, make check + JB gradle + tsc/tests green, cdylib lib-installed). REMAINING operator-gated: (1) install the JB plugin zip from disk (build/distributions/agent-doc-jetbrains-0.2.173-signed.zip) + restart IDEA, and install the VS Code .vsix (editors/vscode/agent-doc-0.2.29.vsix) + reload window — the Kotlin/TS reporter changes need a plugin reinstall, the cdylib FFI is already hot-reloaded. (2) In a live editor, type a NON-ASCII edit (e.g. insert/delete around "café 日本 😀") into a session doc while an agent merge runs; confirm ops.log shows editor_op_recorded kind=insert/delete with BYTE offsets (not UTF-16) and that the concurrent merge aligns to the real edit boundary (no character interleaving / mojibake). The base-hash gate + replay==theirs gate make a misaligned op fall back to the diff-guess safely, so a wrong offset cannot corrupt — verification confirms the ops are ACCEPTED (editor_ops_for_base match), not merely safe.
- [ ] [#fbwireverify] [operator-verify] #agent-doc-bug #fbwireverify Live two-pane make tmux-ci acceptance for #fbwire/#fullboundary Phase 2 (agent-doc submodule 85b173fb; committed+pushed, make check green 4956, clippy clean; NOT installed per STOP-cutting-releases). After make install (cargo install --path . + cargo build --release + agent-doc lib-install) + agent-doc admin recycle at a recycle boundary, drive two live panes: (1) a slow-ACK editor drain must never pile IPC inflight > 1 — confirm ops.log shows convergence_gate_defer lines (unmet=inflight_drained) before each dispatch and NO inflight>1; (2) a wedged (no-ACK) editor must trip the bounded CONVERGENCE_GATE_TIMEOUT_MS (30s) → ERROR-level convergence_gate_force_disk_fallback severity=error + a loadable .agent-doc/playback/<hash>/<cycle>.json artifact, and the drain proceeds (never permanently stuck). Operator-gated: needs two live editor/CLI panes the driving session cannot host.
- [ ] [#supinstallvis] [recommended] #agent-doc-bug #supinstallfeedback follow-up: the supervisor auto-install status (surface_supervisor_auto_install_status, idle_watch.rs) is surfaced ONLY as a transient tmux display-message on the status line, which is easy to miss while the pane scrollback keeps showing the misleading post-exit "Press Enter to restart" keepalive — so an operator watching the pane body still perceives a ~1-min stall. Residual: make the rebuild state visible in the pane BODY/scrollback (not just the status line) without disturbing the keepalive Enter-to-restart reader, e.g. a pane title set + a single non-input-consuming visible line, or render it through the JB/VSCode plugin notification surface (overlaps #qpauseux notification rendering). Decide the safe pane-write mechanism with the operator before implementing (send-keys risks being consumed by the keepalive). Plan TBD.
- [ ] [#1vhn] route dead-shell: positively identify a HARNESS dispatch-ready prompt and treat a bare shell prompt as NOT-ready -> fail closed with claim/restart guidance (or auto-restart) instead of typing into the shell; re-verify the prompt immediately before send; add dead-harness diagnostic + SimWorld. Operator capture supervisor-stderr + scrollback if it recurs. Plan: tasks/agent-doc/plan-route-dispatch-into-crashed-harness.md
- [ ] [#jbrundispatch] OPERATOR-CAPTURE live repro at stall (admin detect for cross-doc pane contamination + ops.log route trace + which pane state failed) to distinguish a real new route/submit bug from already-fixed behavior. Overlaps jbrunstart / startup-miss supersession / qdispatchloss; cannot be fixed blind in route.rs.
- [ ] [#docdriftgrace] OPERATOR-WATCH one live re-baseline cycle: binary should classify mid/between-turn drift by component and silently auto-rebase snapshot/CRDT for unrelated changes, surfacing only structural conflicts on the in-flight prompt/response. Plan: tasks/agent-doc/plan-graceful-doc-drift-resolution.md
- [ ] [#runexitrestart] route/idle-watch should detect a cleanly-exited harness pane (Press Enter to restart) with prompt-bearing work, restart the harness, wait for dispatch-ready, then submit, so a cleanly-exited harness never drops a Run Agent Doc. Two-pane live verify. NOTE the originally-reported silent loss was already fixed in 0.34.18.
- [ ] [#gf5n] verification check under hap7: confirm numbered/ordered-list order survives a merge-during-finalize cycle (the reversed-list symptom was a CRDT merge-corruption artifact, not an independent render bug; resolves when hap7 scoped-merge lands).
- [ ] [#arsm] [operator-verify] #pcwcwarnverify #agent-doc-bug Live-verify the #pcwcwarn post-commit stale-exchange reconcile (agent-doc submodule 5f3fc07d, superproject db7fc3594). Code shipped+pushed+make-check-green (4947). Runs in the binary commit path + editor IPC, so it goes live once the supervisor adopts the new build (admin recycle done). REMAINING (operator-gated, needs a live editor): drive one edit-during-finalize cycle with a stale > Queue prompt blockquote left inside the exchange in a live JB/VS Code buffer; confirm ops.log shows postcommit_exchange_reconciled reason=stale_editor_exchange AND zero recurring postcommit_worktree_check match=false drift on the next cycle (no manual git checkout HEAD needed).
- [ ] [#tb4q] [operator-verify] #qfocsupverify #agent-doc-bug Live-verify #qfocsup supervisor clear-and-continue for [focused-cycle] heads (agent-doc submodule 67829f3f; committed, make check green, NOT installed per STOP-cutting-releases). After make install + agent-doc admin recycle: with a [focused-cycle] head at the queue head and no in-session-drainable work, confirm the supervisor idle-watch force-clears the session (/clear) and re-dispatches the head to a fresh session instead of going idle. Proof: session-check ui_outcome=deferred_for_supervisor_drain on the yielding turn, then ops.log context_reset + go_drain_dispatch + a fresh agent draining the head.
- [ ] [#qcm1verify] [operator-verify] #agent-doc-bug Install + live-verify the #qnodemerge1 component-scoped merge (agent-doc submodule 52458f47). Code shipped+pushed, make check green (4853). Operator-gated remainder (mid-session install causes finalize-client-newer drift; STOP-cutting-releases gate): at a supervisor-recycle boundary run make install (cargo install --path . + cargo build --release + agent-doc lib-install), agent-doc admin recycle, then drive one edit-during-finalize cycle (type into a queue item while the agent writes an exchange response) and confirm ZERO cross-component splice + no console-output-in-queue corruption.
- [ ] [#1jec] [operator-verify] #yzerverify #agent-doc-bug Live-verify the #yzer/#evmhplugin plugin reconnect re-read (agent-doc 0.34.28 / submodule f6b8ae17, plugin 0.2.172). Code shipped+pushed: binary FFI agent_doc_reconnect_buffer_decision (disk==HEAD via ffi_show_head + buffer==prior-commit via ffi_show_prior_blobs) + pure decide_reconnect_buffer + PatchWatcher.registerRoot reconcile-on-connect (applyReconnectReread) + SimWorld coverage; make check green (4832). Operator-gated remainder (install is unsafe mid-session = finalize-client-newer drift): at a supervisor-recycle boundary run make install (cargo install --path . + cargo build --release + agent-doc lib-install), reinstall plugin 0.2.172, agent-doc admin recycle. Then repro: open a session doc in JB, kill/restart the supervisor so the binary commits while the plugin is disconnected, and on reconnect confirm ops.log shows 'reconnect_buffer_decision decision=reread_disk #yzer' and the editor buffer == HEAD with NO File Cache Conflict; a buffer holding genuine unsaved user edits must instead log decision=keep_buffer and be preserved (editor wins).
<!-- /agent:backlog -->

## Review
 
<!-- agent:review -->
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
