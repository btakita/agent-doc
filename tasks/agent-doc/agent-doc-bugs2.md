---
agent_doc_session: b26b9957-8de6-4eae-bb98-5d7fe4d6b781
agent: claude
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

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7a9b103bd5b68223dac05a4c449ac03899aba53d6022e82100578b5b0-20260622-042851.md`*

Compacted content:
- Archived 6 response topic(s): #opslogtrack verified live; make the transport healthy + advance the queue; do [#qheadstrike]; 3 more
- Prior summary/context: ❯ *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7a9b103bd5b68223dac...

### Re: Validate/invalidate operator-verify items from the logs — opus-4-8

> **Queue prompt:**
>
> Use the logs to validate or invalidate any operator verify items in the queue.


> **Queue prompt:** Use the logs to validate or invalidate any operator verify items in the queue.

Grepped `.agent-doc/logs/ops.log` (3.0M lines) for each item's declared proof marker, separating real timestamp-prefixed **event** lines from backlog **prose** that merely names a marker. Results:

**VALIDATED (closing):**

- **#k7mf / #g9d7verify** — `run_ipc_editorless_disk_fallback transport=disk_fallback editor_endpoint=absent` fired 3× (2026-06-21 18:05/18:18/18:51Z) with `recovery=editorless_disk_write` — exactly the required fields, and crucially **not** `recovery=retry_without_disk_write`. Editorless (no live editor) is the operator-gated scenario; the marker proves the run_ipc no-delivery bail routes content_ours to the guarded disk write. `--done`.
- **#routeretest** — 115 real `dispatch_inject … attempt=1` events (incl. this doc, 2026-06-22 04:28:58Z, `transport=direct_pane attempt=1`), **zero** `attempt=2+` restacks, and the dispatches drove committed turns. No escalating re-send loop, no stray `/clear`. The reported stall/no-submit/storm symptoms do not reproduce on the fresh binary in the logs. `--done` (log-based; re-file if a *live* JB pane still stalls).

**NOT validated from logs (genuinely operator-gated — stay open):**

- **#1jec / #yzerverify** — `reconnect_buffer_decision` path is live (110 events) and the `decision=keep_buffer` branch fired 1× (the "preserve genuine unsaved edits" half), but `decision=reread_disk` **never** fired — the stale-buffer-vs-HEAD repro (kill supervisor while plugin disconnected) wasn't exercised. Mechanism wired; reread branch unproven.
- **#arsm / #pcwcwarnverify** — `postcommit_exchange_reconciled reason=stale_editor_exchange` fired 2× (mechanism confirmed) but both `transport=disk` (editorless), not via a live JB/VS Code buffer; and one transient `postcommit_worktree_check match=false` (04:20:39Z) still appeared before self-healing to `match=true` 1s later. The live-editor stale-blockquote proof is unmet.
- **#tb4q / #qfocsupverify** — zero real `go_drain_dispatch` events and zero `deferred_for_supervisor_drain` session-check outcomes (only backlog prose matched). The [focused-cycle] supervisor clear-and-continue path was never exercised; needs make install + recycle + a [focused-cycle] head.
- **#qcm1verify** — no live edit-during-finalize cycle in the logs to evaluate cross-component splice; operator-gated.

Net: 2 closed from log evidence, 4 remain operator-verify (need a live editor/pane the logs can't substitute for).

### Re: do [#fbwire] — convergence-gate Phase 2 wiring — opus-4-8

> **Queue prompt:**
>
> :pushpin: do [#fbwire]


Wired the Phase-1 `convergence_gate` decision core into the live supervisor inter-item dispatch (`start/idle_watch.rs`, agent-doc submodule `85b173fb`, superproject `6516b8fc3` — committed + pushed; **not installed**, per STOP-cutting-releases).

**What it does.** At the `IdleQueueDrainDecision::Dispatch` arm, before injecting item N+1's trigger, it gathers four facts from live state and gates on a quiescent close of item N:
- `committed` — `cycle_state::load().phase == Committed` (defaults `true` when there's no cycle state, so a fresh/first drain is never blocked)
- `editor_converged` — disk worktree == `HEAD` (the `#pcwc` `normalize_for_replay_hash` ground truth; non-git / unreadable `HEAD` defaults `true` so it can never wedge)
- `inflight` — `ipc_socket::inflight_connection_handlers() == 0`
- `actor_idle` — `!actor_state_is_busy_or_starting()`

`Dispatch` → proceed; `Defer{unmet}` → start/keep a per-boundary timer, log `convergence_gate_defer`, skip the tick (caps inflight at ~1, closing the `content_ours` / `postcommit_worktree_check match=false` / `inflight=5 send_failed` windows); `ForceDiskFallback{unmet}` → editor IPC wedged past `CONVERGENCE_GATE_TIMEOUT_MS` (30s/~60 ticks): persist a replayable `ConvergencePlayback` + emit the **loud** `convergence_gate_force_disk_fallback severity=error … playback=<path>` line, then proceed so the drain is **never permanently stuck** (disk reconcile stays owned by the existing postcommit `disk_after_failed_editor_refresh` path — the idle watch does not author a document write).

**Safety.** A wrong `editor_converged` fact can at worst cause a bounded 30s defer + one loud fallback — never a deadlock. Composes with (doesn't replace) the drain-owner lease (`#kp5z`) and clear-settle guards.

**Verification.** `make check` green (4956 tests), clippy clean (`cargo clippy -- -D warnings` exit 0). 5 new tests: editorless/non-git default-true, disk-vs-`HEAD` tracking, slow-ACK never dispatches while inflight > 0, wedged-editor trips force-disk after timeout, loud-error + loadable-playback artifact. SPEC L36 already documented this consumer, so no SPEC change.

The agent-doable portion is complete, so I'm closing `#fbwire`. The genuinely operator-gated remainder — the live **two-pane `make tmux-ci`** acceptance (real slow-ACK + wedged-editor), which can't run from this driving session — is filed as `#fbwireverify`; it goes live after `make install` + `admin recycle` at a recycle boundary.

### Re: do [#j9ja] — SUCCESS ops.log markers for operator auto-verify — opus-4-8

> **Queue prompt:**
>
> :pushpin: do [#j9ja]


Audited the four proof sites #j9ja names; two were actionable, two are blocked on unbuilt parent features.

**Shipped this cycle** (agent-doc submodule `2367d883`, superproject `a2e989d3f`; committed + pushed, **not installed**):
- `convergence_gate_converged_dispatch` — new distinctive SUCCESS marker in the `#fbwire` Phase 2 convergence gate (`idle_watch.rs` Dispatch arm). Emitted **only when the gate had been deferring** (the meaningful "boundary converged after waiting on the editor/IPC" signal), so the `#fbwireverify` slow-ACK live test auto-verifies (`convergence_gate_defer …` → `convergence_gate_converged_dispatch waited_ms=…`) while the steady-state quiescent dispatch stays quiet. `make check` green (4956), clippy clean.

**Already present** (no work needed): `route_dispatch_submit_recycle_settle` (route/dispatch.rs:831, the `#jbdisprecycle` route bucket), plus the two #j9ja already lists — `auto_start_dispatch_ready_confirmed`, `queue_paused_continuation_guidance_emitted`.

**Blocked — code sites don't exist yet** (gating #j9ja):
- `agent_restart_performed` — only `harness_change_detected` + `agent_restart_boundary_gate` (with `note=phase1b_execution_pending`) exist today; the restart *execution* is `#agentreloadrestart` Phase 1b, not built. The marker must be emitted by that execution path when it lands.
- restart-agent menu-invoke marker — `#restartagentmenu` (`RestartAgentAction.kt`) isn't built.

I gated `#j9ja` rather than closing it: the two remaining markers belong inside `#agentreloadrestart` Phase 1b and `#restartagentmenu` and should be added when those features are implemented. Un-gates when they land.
<!-- agent:boundary:3e5ad1a2 -->
<!-- /agent:exchange -->

## Queue

<!-- agent:queue priority preset="#spec-test-build-install-commit-push" priority go -->
- ~~Use the logs to validate or invalidate any operator verify items in the queue.~~
- ~~:pushpin: do [#fbwire]~~
- ~~:pushpin: do [#j9ja]~~
- :pushpin: do [#qnodemerge4wire]
- :pushpin: do [#tb4q]
- :pushpin: do [#agentreloadrestart]
- :pushpin: do [#restartagentmenu]
- :pushpin: Add `Stop Agent` and `Restart Agent` in JB + VSCode menus.
- :pushpin: JB `Run Agent Doc` on monsterrodholders.md stalled after restarting the agent in both Claude Code + Codex.
- :pushpin: `agent-doc tasks/monsterrodholders.md` was added to the codex input but not submitted. Whatever wrote the prompt should also loop to check the input and submit it if not submitted.
- :pushpin: As I was writing queue items during a turn, an extra `/clear\n/agent-doc tasks/agent-doc/agent-doc-bugs2.md\n/clear` was written into the input and submitted.
- :pushpin: do [#qcm1verify]
- :pushpin: do [#s81q]: Rename "Restart Supervisor" to "Recycle Supervisor". Add Restart command alongside "Recycle Supervisor"
- :pushpin: do [#qstrikenote]
- :pushpin: [#pendaddorder]
- :pushpin: JS `Clear Session Context` then JB `Run Agent Doc` does nothing. Subsequent JB `Run Agent Doc` eventually dispatches...it seems there's a delay before working. Please enable JB `Run Agent Doc` to dispatch immediately.
- :pushpin: JS `Clear Session Context` then JB `Run Agent Doc` sometimes has the dispatch storm (multiple duplicate prompts) and no submit.
- :pushpin: do [#arsm]
- :pushpin: do [#editorbufwin]
- :pushpin: do [#jbdisprecycle]
- :pushpin: do [#sqedit-race]: Validate if the fix is successful by looking at the logs. No `queue_stale_noise_lines` means success.
- :pushpin: do [#qpauseux]
- :pushpin: do [#8bfz]
- :pushpin: do [#cyh0]
- :pushpin: do [#1vhn]
- :pushpin: do [#jbrundispatch]
- :pushpin: do [#docdriftgrace]
- :pushpin: do [#runexitrestart]
- :pushpin: do [#gf5n]
- :pushpin: do [#supinstallvis]
- :pushpin: do [#1jec]
- do [#fbwireverify]
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
- [ ] [#fbwireverify] [operator-verify] #agent-doc-bug #fbwireverify Live two-pane make tmux-ci acceptance for #fbwire/#fullboundary Phase 2 (agent-doc submodule 85b173fb; committed+pushed, make check green 4956, clippy clean; NOT installed per STOP-cutting-releases). After make install (cargo install --path . + cargo build --release + agent-doc lib-install) + agent-doc admin recycle at a recycle boundary, drive two live panes: (1) a slow-ACK editor drain must never pile IPC inflight > 1 — confirm ops.log shows convergence_gate_defer lines (unmet=inflight_drained) before each dispatch and NO inflight>1; (2) a wedged (no-ACK) editor must trip the bounded CONVERGENCE_GATE_TIMEOUT_MS (30s) → ERROR-level convergence_gate_force_disk_fallback severity=error + a loadable .agent-doc/playback/<hash>/<cycle>.json artifact, and the drain proceeds (never permanently stuck). Operator-gated: needs two live editor/CLI panes the driving session cannot host.
- [ ] [#supinstallvis] [recommended] #agent-doc-bug #supinstallfeedback follow-up: the supervisor auto-install status (surface_supervisor_auto_install_status, idle_watch.rs) is surfaced ONLY as a transient tmux display-message on the status line, which is easy to miss while the pane scrollback keeps showing the misleading post-exit "Press Enter to restart" keepalive — so an operator watching the pane body still perceives a ~1-min stall. Residual: make the rebuild state visible in the pane BODY/scrollback (not just the status line) without disturbing the keepalive Enter-to-restart reader, e.g. a pane title set + a single non-input-consuming visible line, or render it through the JB/VSCode plugin notification surface (overlaps #qpauseux notification rendering). Decide the safe pane-write mechanism with the operator before implementing (send-keys risks being consumed by the keepalive). Plan TBD.
- [ ] [#agentreloadrestart] restart-agent Phase 1: supervisor restart re-resolves harness from current frontmatter (agent claude->opencode takes effect on restart) + boundary auto-restart when resolved harness differs, behind agent_doc_agent_change_restart (default ON). Log harness_change_detected old/new + agent_restart_boundary_gate + agent_restart_performed. SimWorld + make tmux-ci. Plan: tasks/agent-doc/plan-restart-agent.md
- [ ] [#restartagentmenu] restart-agent Phase 2 (after agentreloadrestart): RestartAgentAction.kt JB menu cmd mirroring RestartSupervisorProcessAction.kt + plugin.xml registration + TerminalUtil restart wiring; bump pluginVersion; build signed+unsigned. Log action invocation + resolved harness. Decide alongside-vs-replace Restart Supervisor Process with operator. Plan: tasks/agent-doc/plan-restart-agent.md
- [ ] [#editorbufwin] merge-core: P1 the #pcwc post-commit reconcile must preserve unconsumed user queue additions on disk (do NOT overwrite the disk addition with HEAD); P2 auto-recover the commit (automate the manual reset --from-current); P4 refresh the stale supervisor CRDT enabling the drift. Plan: tasks/agent-doc/plan-editor-buffer-wins-queue-merge.md
- [ ] [#qnodemerge4wire] Phase 4: thin JB DocumentListener.documentChanged + VS Code onDidChangeTextDocument reporters that split a replacement into delete+insert and call the agent_doc_record_editor_op FFI; verify byte-offset/Yrs unicode-offset semantics on non-ASCII edits. Needs plugin build. Plan: tasks/agent-doc/plan-percell-crdt-merge-isolation.md (Phase 4)
- [ ] [#jbdisprecycle] OPERATOR-TEST (code R1-R4 shipped submodule d21079b7): at a recycle boundary fire JB Run Agent Doc on a 2nd doc during the hot-reload burst; confirm a single dispatch_start_proof WITH submit and no stranded start_session failed. ops.log marker: route_dispatch_submit_recycle_settle.
- [ ] [#sqedit-race] OPERATOR-TEST Phase 4 (Phases 1-3 shipped 3c4c8bbb): after make install + admin recycle, queue prune-noise this doc; confirm queue_stale_noise_lines drops to 0 and sticks across a later preflight. Plan: tasks/agent-doc/plan-supervisor-direct-queue-edit-race.md
- [ ] [#qpauseux] OPERATOR-TEST (plugin render; needs JB plugin build): install updated plugin, verify a paused-queue block renders as an info/action notification with a Resume button, and the churn-stop/no-submit variant surfaces a restart-supervisor/resume action instead of silence (not a raw red Error toast).
- [ ] [#8bfz] single live JB plugin consumer per document (per-doc owner lease; non-owners defer patch apply + saveDocument) so concurrent windows cannot race into File Cache Conflicts. Plugin PatchWatcher + binary lease primitive; operator multi-window live test.
- [ ] [#cyh0] OPERATOR-CAPTURE: the remaining File Cache Conflict has an unidentified trigger (hot-path disk writes are already listener-guarded). Capture which exact action raises the dialog (agent-doc cmd + ops.log line, e.g. write_authority action=... transport=disk). Likely: ack-content sidecar write, VCS-refresh/save-document signal, or an editor-IPC convergence the plugin surfaces as conflict.
- [ ] [#1vhn] route dead-shell: positively identify a HARNESS dispatch-ready prompt and treat a bare shell prompt as NOT-ready -> fail closed with claim/restart guidance (or auto-restart) instead of typing into the shell; re-verify the prompt immediately before send; add dead-harness diagnostic + SimWorld. Operator capture supervisor-stderr + scrollback if it recurs. Plan: tasks/agent-doc/plan-route-dispatch-into-crashed-harness.md
- [ ] [#jbrundispatch] OPERATOR-CAPTURE live repro at stall (admin detect for cross-doc pane contamination + ops.log route trace + which pane state failed) to distinguish a real new route/submit bug from already-fixed behavior. Overlaps jbrunstart / startup-miss supersession / qdispatchloss; cannot be fixed blind in route.rs.
- [ ] [#docdriftgrace] OPERATOR-WATCH one live re-baseline cycle: binary should classify mid/between-turn drift by component and silently auto-rebase snapshot/CRDT for unrelated changes, surfacing only structural conflicts on the in-flight prompt/response. Plan: tasks/agent-doc/plan-graceful-doc-drift-resolution.md
- [ ] [#runexitrestart] route/idle-watch should detect a cleanly-exited harness pane (Press Enter to restart) with prompt-bearing work, restart the harness, wait for dispatch-ready, then submit, so a cleanly-exited harness never drops a Run Agent Doc. Two-pane live verify. NOTE the originally-reported silent loss was already fixed in 0.34.18.
- [ ] [#gf5n] verification check under hap7: confirm numbered/ordered-list order survives a merge-during-finalize cycle (the reversed-list symptom was a CRDT merge-corruption artifact, not an independent render bug; resolves when hap7 scoped-merge lands).
- [ ] [#arsm] [operator-verify] #pcwcwarnverify #agent-doc-bug Live-verify the #pcwcwarn post-commit stale-exchange reconcile (agent-doc submodule 5f3fc07d, superproject db7fc3594). Code shipped+pushed+make-check-green (4947). Runs in the binary commit path + editor IPC, so it goes live once the supervisor adopts the new build (admin recycle done). REMAINING (operator-gated, needs a live editor): drive one edit-during-finalize cycle with a stale > Queue prompt blockquote left inside the exchange in a live JB/VS Code buffer; confirm ops.log shows postcommit_exchange_reconciled reason=stale_editor_exchange AND zero recurring postcommit_worktree_check match=false drift on the next cycle (no manual git checkout HEAD needed).
- [ ] [#pendaddorder] Repeatable `--pending-add` block-prepends in flag order instead of composing as the documented "at the beginning (repeatable)" per-item prepends, so ONE `agent-doc write` with N `--pending-add` flags lands items REVERSED vs N sequential writes, and the agent:queue mirror inherits the reversal. Live repro 2026-06-21 equityfundingsource.md: `--pending-add [#msgmail4] --pending-add [#msgmail3] --pending-add [#msgmail2] --pending-add [#msgmail1]` -> backlog AND queue top-down = msgmail4,3,2,1 (caller expected 1,2,3,4 per the per-item-prepend mental model). Plan (DO NOT IMPLEMENT): (1) define ONE semantics for repeatable --pending-add and document it (recommend preserve flag order as final top-down order = what you read is what you get; route strict-order callers to --pending-add-after chains); (2) update help text + SKILL.md/pending-ops "put new items at the beginning" to state the multi-flag order; (3) make the agent:queue mirror order deterministically match the backlog order; (4) add a test asserting multi-flag --pending-add order + queue-mirror parity.
- [ ] [#qstrikenote] [recommended] #agent-doc-bug #qstrikeexplain Phase 1 follow-up: explicit deterministic explanation note when #ftstrike auto-strikes a free-text queue head. Phase 2 (do not strike in-flight/mid-typed heads; baseline-gated) SHIPPED agent-doc submodule 06933b72. The strike already requires the response to quote the struck head in a blockquote; add a binary-injected one-line auto-struck note so the operator sees an explicit statement. Decide deterministic-injection vs a finalize-field per all-deterministic-in-binary. Plus a SimWorld case that a strike emits the note. Plan: tasks/agent-doc/plan-queue-strike-explanation.md (Phase 1 + Phase 3 SimWorld).
- [ ] [#s81q] [recommended] #agent-doc-bug #stopvscodemenu Extend #restartagentmenu (JB Restart Agent; plan tasks/agent-doc/plan-restart-agent.md) per operator request to also add a Stop Agent command and mirror BOTH Stop+Restart into the VSCode extension menus. Delta beyond #restartagentmenu: (1) StopAgentAction.kt + plugin.xml registration on the JB side; (2) VSCode command-palette + editor/title menu contributions for stop+restart wired to the same TerminalUtil/route paths. Decide alongside-vs-replace the existing Restart Supervisor Process action; do not remove without operator confirmation. Bump pluginVersion + build signed/unsigned zips (gated by STOP-cutting-releases). Confirm scope before implementing.
- [ ] [#tb4q] [operator-verify] #qfocsupverify #agent-doc-bug Live-verify #qfocsup supervisor clear-and-continue for [focused-cycle] heads (agent-doc submodule 67829f3f; committed, make check green, NOT installed per STOP-cutting-releases). After make install + agent-doc admin recycle: with a [focused-cycle] head at the queue head and no in-session-drainable work, confirm the supervisor idle-watch force-clears the session (/clear) and re-dispatches the head to a fresh session instead of going idle. Proof: session-check ui_outcome=deferred_for_supervisor_drain on the yielding turn, then ops.log context_reset + go_drain_dispatch + a fresh agent draining the head.
- [ ] [#qcm1verify] [operator-verify] #agent-doc-bug Install + live-verify the #qnodemerge1 component-scoped merge (agent-doc submodule 52458f47). Code shipped+pushed, make check green (4853). Operator-gated remainder (mid-session install causes finalize-client-newer drift; STOP-cutting-releases gate): at a supervisor-recycle boundary run make install (cargo install --path . + cargo build --release + agent-doc lib-install), agent-doc admin recycle, then drive one edit-during-finalize cycle (type into a queue item while the agent writes an exchange response) and confirm ZERO cross-component splice + no console-output-in-queue corruption.
- [ ] [#1jec] [operator-verify] #yzerverify #agent-doc-bug Live-verify the #yzer/#evmhplugin plugin reconnect re-read (agent-doc 0.34.28 / submodule f6b8ae17, plugin 0.2.172). Code shipped+pushed: binary FFI agent_doc_reconnect_buffer_decision (disk==HEAD via ffi_show_head + buffer==prior-commit via ffi_show_prior_blobs) + pure decide_reconnect_buffer + PatchWatcher.registerRoot reconcile-on-connect (applyReconnectReread) + SimWorld coverage; make check green (4832). Operator-gated remainder (install is unsafe mid-session = finalize-client-newer drift): at a supervisor-recycle boundary run make install (cargo install --path . + cargo build --release + agent-doc lib-install), reinstall plugin 0.2.172, agent-doc admin recycle. Then repro: open a session doc in JB, kill/restart the supervisor so the binary commits while the plugin is disconnected, and on reconnect confirm ops.log shows 'reconnect_buffer_decision decision=reread_disk #yzer' and the editor buffer == HEAD with NO File Cache Conflict; a buffer holding genuine unsaved user edits must instead log decision=keep_buffer and be preserved (editor wins).
<!-- /agent:backlog -->

## Review
 
<!-- agent:review -->
- [/] [#j9ja] [/] [blocked] #optverify SUCCESS ops.log markers so operator live tests auto-verify. SHIPPED this cycle: convergence_gate_converged_dispatch (agent-doc submodule 2367d883 — #fbwire Phase 2 success-after-defer in idle_watch.rs, emitted only when the gate had been deferring; auto-verifies #fbwireverify slow-ACK). Already exists: route_dispatch_submit_recycle_settle (route/dispatch.rs:831), auto_start_dispatch_ready_confirmed, queue_paused_continuation_guidance_emitted. BLOCKED (code sites do not exist yet): agent_restart_performed — add inside #agentreloadrestart Phase 1b restart EXECUTION (today only harness_change_detected + agent_restart_boundary_gate note=phase1b_execution_pending exist); restart-agent menu-invoke marker — add inside #restartagentmenu RestartAgentAction. Un-gate + close when those two parent features land and their markers are added.
<!-- /agent:review -->

<!-- agent:icebox -->
<!-- /agent:icebox -->

## Completed / Reaped

<!-- agent:done archive=tasks/agent-doc-bugs2.done.md -->

<!-- completed work archived in tasks/agent-doc-bugs2.done.md -->
<!-- /agent:done -->
