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

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7a9b103bd5b68223dac05a4c449ac03899aba53d6022e82100578b5b0-20260621-213916.md`*

Compacted content:
- Archived 1 response topic(s): do [#pcwcwarn]
- Prior summary/context: prior compacted content: Archived 14 response topic(s): #1jec; do [#qauthorderpin]; harness switch to claude; 11 more

### Re: JB Run Agent Doc "stalled without feedback" during auto-install — opus-4-8

> **Queue prompt:** JB `Run Agent Doc` stalled without feedback — pane showed `claude exited cleanly. / Press Enter to restart, or 'q' to exit.` repeated, idle-queue watch reconciled stale busy→ready, then `supervisor auto-install: rebuilding + installing agent-doc … Compiling agent-doc v0.34.35`.

Diagnosed and fixed (`#supinstallfeedback`, agent-doc submodule `cf7347a9`, superproject `baf6a8ec2`).

**What actually happened** (this log is from the prior cycle's own closeout): after my `#pcwcwarn` commit landed, the supervisor's idle-watch detected the source was newer than the installed binary and ran the **dogfood auto-install** — a ~1-minute blocking `cargo install`. During that minute the pane still showed the post-`claude exited cleanly` keepalive `Press Enter to restart, or 'q' to exit`, and **all rebuild progress went to the redirected supervisor stderr / session log** — never to the visible pane. So the keepalive read as a stall waiting on a keypress. Nothing was actually wedged; it was rebuilding.

**Fix:** the auto-install now surfaces its phase on the owned tmux pane via `display-message`, mirroring the existing managed-capability-proof status surface (diagnostics stay out of the child pane transcript; user-visible status goes to the tmux status line). The `Started` line persists (`-d 0`) for the whole compile and says **"rebuilding the freshly-committed binary (~1 min) — do NOT press Enter; the supervisor auto-restarts when the build finishes"**; `Succeeded`/`Failed` use a bounded display time. Best-effort + fail-open (no owned pane → stderr, unchanged).

Tests: 1 unit (message wording); `make check` green (4948). Prior `#pcwcwarn` CI (`5f3fc07d`) **completed green**.

> **Operator live-verify:** next time a commit triggers the supervisor auto-install, the tmux status line should show the "rebuilding … do NOT press Enter" message instead of a silent pane. Filed as `#supinstallfeedbackverify`.

Note: the rest of this queue head's siblings (dispatch storm after `Clear Session Context`, trigger typed-but-not-submitted, `Stop/Restart Agent` menu items, `Recycle Supervisor` rename) are separate tracked items further down the queue — draining them in order.

### Re: clear all gated review items → actionable backlog replacements — opus-4-8

Done — the review is empty and the plan no longer carries any gates. Committed `38b83be05`.

**What I did:** converted all **22 gated review items** (`[/]`) into **ungated, actionable backlog items** (`[ ]`) and cleared `agent:review`. Each replacement preserves its `#id`, its plan path, and — per "I will test, file bugs, and you will check the logs" — its concrete ops.log success marker so your live test proves/disproves it:

- **Operator-test now (code shipped, just needs your live run):** `#jbdisprecycle` (route_dispatch_submit_recycle_settle), `#rdypoll-deploy` (dispatch_inject attempt=1), `#wd40` (supervisor_recycle_yield_requested reason=state_flush_drain), `#qpausemix-verify` (queue_paused_continuation_guidance_emitted), `#tsiftmdcrash-verify` (capability_proof_live_child_preserved), `#sqedit-race` P4 (queue_stale_noise_lines=0), `#gf5n`, `#docdriftgrace`, `#runexitrestart`.
- **Operator-capture (need a live repro/log capture from you):** `#cyh0` (which action raises File Cache Conflict), `#jbrundispatch`, `#1vhn`.
- **Still agent code-work (queued, not gated):** `#editorbufwin`, `#hap7`, `#fbwire`, `#qheadstrike`, `#qnodemerge4wire` P4, `#agentreloadrestart` + `#restartagentmenu` (restart-agent, unshipped — these need their logging shipped first, tracked in `#j9ja`).

**JB plugin updates you'll need to install** (for the items whose proof needs plugin code): `#qpauseux` (paused-queue notification render), `#8bfz` (single-owner lease), `#restartagentmenu` (Restart Agent menu), `#qnodemerge4wire` P4 (DocumentListener reporters). I'll flag the exact build when each lands; none are shipped yet.

**Also this cycle:** noted your `#1jec` strike (resolved) and preserved the `:pushpin:` position-locks (`#qauthorder`). The answered JB-stall head (`#supinstallfeedback`, shipped `cf7347a9`) stays at the queue head only because last cycle's `content_ours` drift recovery reverted its strike — no re-work needed.
<!-- agent:boundary:e0c134a4 -->
<!-- /agent:exchange -->

## Queue

<!-- agent:queue priority preset="#spec-test-build-install-commit-push" priority go -->
---
:round_pushpin: JB `Run Agent Doc` stalled without feedback
```
claude exited cleanly.
Press Enter to restart, or 'q' to exit.
[start] stderr redirected to /home/brian/work/btakita/agent-loop/.agent-doc/logs/supervisor-stderr.log for claude TUI
[agent-doc] idle-queue watch: reconciled stale busy actor to ready from idle pane evidence (no pane kill)

claude exited cleanly.
Press Enter to restart, or 'q' to exit.
[start] stderr redirected to /home/brian/work/btakita/agent-loop/.agent-doc/logs/supervisor-stderr.log for claude TUI
[agent-doc] idle-queue watch: reconciled stale busy actor to ready from idle pane evidence (no pane kill)
[agent-doc] idle-queue watch: reconciled stale busy actor to ready from idle pane evidence (no pane kill)
[agent-doc] idle-queue watch: reconciled stale busy actor to ready from idle pane evidence (no pane kill)
[agent-doc] supervisor auto-install: rebuilding + installing agent-doc from /home/brian/work/btakita/agent-loop/src/agent-doc so the next queue item runs on the freshly-committed source
   Compiling agent-doc v0.34.35 (/home/brian/work/btakita/agent-loop/src/agent-doc)
```
---
- ~~[#1jec]~~
- Add a command to explicitly sync IPC wedge issues. If wedge issues can be handled entirely in the binary, implement the binary logic that also logs a warning.
- All log output should include a timestamp...especially ops.log. ops.log could also include the name of the agent document for easier tracking when working with multiple documents.
- :pushpin: do [#tb4q]
- :pushpin: do [#k7mf]
- :pushpin: Add `Stop Agent` and `Restart Agent` in JB + VSCode menus.
- :pushpin: JB `Run Agent Doc` on monsterrodholders.md stalled after restarting the agent in both Claude Code + Codex.
- :pushpin: `agent-doc tasks/monsterrodholders.md` was added to the codex input but not submitted. Whatever wrote the prompt should also loop to check the input and submit it if not submitted.
- :pushpin: As I was writing queue items during a turn, an extra `/clear\n/agent-doc tasks/agent-doc/agent-doc-bugs2.md\n/clear` was written into the input and submitted.
- :pushpin: do [#qcm1verify]
- :pushpin: do [#s81q]: Rename "Restart Supervisor" to "Recycle Supervisor". Add Restart command alongside "Recycle Supervisor"
- :pushpin: do [#qstrikenote]
- :pushpin: do [#routeretest]
- :pushpin: [#pendaddorder]
- :pushpin: JS `Clear Session Context` then JB `Run Agent Doc` does nothing. Subsequent JB `Run Agent Doc` eventually dispatches...it seems there's a delay before working. Please enable JB `Run Agent Doc` to dispatch immediately.
- :pushpin: JS `Clear Session Context` then JB `Run Agent Doc` sometimes has the dispatch storm (multiple duplicate prompts) and no submit.
- :pushpin: do [#arsm]
- do [#agentreloadrestart]
- do [#restartagentmenu]
- do [#j9ja]
- do [#editorbufwin]
- do [#hap7]
- do [#fbwire]
- do [#qheadstrike]
- do [#qnodemerge4wire]
- do [#jbdisprecycle]
- do [#sqedit-race]
- do [#qpauseux]
- do [#8bfz]
- do [#cyh0]
- do [#1vhn]
- do [#jbrundispatch]
- do [#docdriftgrace]
- do [#runexitrestart]
- do [#gf5n]
- do [#supinstallvis]
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
- [ ] [#supinstallvis] [recommended] #agent-doc-bug #supinstallfeedback follow-up: the supervisor auto-install status (surface_supervisor_auto_install_status, idle_watch.rs) is surfaced ONLY as a transient tmux display-message on the status line, which is easy to miss while the pane scrollback keeps showing the misleading post-exit "Press Enter to restart" keepalive — so an operator watching the pane body still perceives a ~1-min stall. Residual: make the rebuild state visible in the pane BODY/scrollback (not just the status line) without disturbing the keepalive Enter-to-restart reader, e.g. a pane title set + a single non-input-consuming visible line, or render it through the JB/VSCode plugin notification surface (overlaps #qpauseux notification rendering). Decide the safe pane-write mechanism with the operator before implementing (send-keys risks being consumed by the keepalive). Plan TBD.
- [ ] [#agentreloadrestart] restart-agent Phase 1: supervisor restart re-resolves harness from current frontmatter (agent claude->opencode takes effect on restart) + boundary auto-restart when resolved harness differs, behind agent_doc_agent_change_restart (default ON). Log harness_change_detected old/new + agent_restart_boundary_gate + agent_restart_performed. SimWorld + make tmux-ci. Plan: tasks/agent-doc/plan-restart-agent.md
- [ ] [#restartagentmenu] restart-agent Phase 2 (after agentreloadrestart): RestartAgentAction.kt JB menu cmd mirroring RestartSupervisorProcessAction.kt + plugin.xml registration + TerminalUtil restart wiring; bump pluginVersion; build signed+unsigned. Log action invocation + resolved harness. Decide alongside-vs-replace Restart Supervisor Process with operator. Plan: tasks/agent-doc/plan-restart-agent.md
- [ ] [#j9ja] Add distinctive SUCCESS ops.log markers at the remaining gated proof sites so operator live tests auto-verify (#optverify): agent_restart_performed (agentreloadrestart), restart-agent menu invoke (restartagentmenu), record_force_disk_fallback (fbwire Phase 2), and the route/plugin bucket. Landed already: auto_start_dispatch_ready_confirmed, queue_paused_continuation_guidance_emitted.
- [ ] [#editorbufwin] merge-core: P1 the #pcwc post-commit reconcile must preserve unconsumed user queue additions on disk (do NOT overwrite the disk addition with HEAD); P2 auto-recover the commit (automate the manual reset --from-current); P4 refresh the stale supervisor CRDT enabling the drift. Plan: tasks/agent-doc/plan-editor-buffer-wins-queue-merge.md
- [ ] [#hap7] merge-core: scope the CRDT/semantic merge to per-structural-node (not whole-component) so a concurrent operator edit to one queue item cannot duplicate adjacent structure; when operator deletes structure agent content targets, note it in agent:exchange instead of merging. Plan: tasks/agent-doc/plan-scoped-crdt-merge-no-structural-duplication.md
- [ ] [#fbwire] Phase 2: wire convergence_gate_decision into the supervisor idle-queue-watch inter-item dispatch (gather committed/editor-converged/ipc-inflight==0/actor-idle; defer next trigger until Dispatch; on ForceDiskFallback do real --force-disk + record_force_disk_fallback). SimWorld + make tmux-ci. Plan: tasks/agent-doc/plan-fullboundary-convergence-gate.md
- [ ] [#qheadstrike] strike ALL answered free-text queue heads (not just one/cycle), move the strike onto the committed seam so force-disk/reset->commit recovery paths converge it, and normalize head matching (pushpin glyph + item split). Plan: tasks/agent-doc/plan-freetext-queue-head-autostrike.md
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
- [ ] [#routeretest] [operator-verify] #agent-doc-bug Live re-test the route/dispatch/stall reports on the POST-RECYCLE fresh binary (06933b72, installed + admin-recycled 2026-06-21). The reported symptoms — JB Run Agent Doc stalled without feedback; agent-doc trigger typed into the Codex/Claude input but not submitted; stray /clear+trigger injected — are ALREADY implemented in shipped code: route/dispatch.rs not_dispatched full-trigger re-send (while acceptance.not_dispatched, ~L856, #jbrundispatch directive 2), send_direct_pane_enter_resubmit_until_stable Enter-resubmit loop (~L779/895), and the dispatch_inject attempt=N ops.log marker (#rdypoll). The operator was most likely observing these on the STALE pre-06933b72 supervisor binary replaced this session. Verify on the fresh binary: drive a JB/Codex Run Agent Doc dispatch and confirm ops.log shows a single dispatch_inject attempt=1 + a submitted trigger (no stacked un-submitted copies, no stray /clear). If a symptom persists on the fresh binary, it is a genuine residual gap to dig into.
- [ ] [#qstrikenote] [recommended] #agent-doc-bug #qstrikeexplain Phase 1 follow-up: explicit deterministic explanation note when #ftstrike auto-strikes a free-text queue head. Phase 2 (do not strike in-flight/mid-typed heads; baseline-gated) SHIPPED agent-doc submodule 06933b72. The strike already requires the response to quote the struck head in a blockquote; add a binary-injected one-line auto-struck note so the operator sees an explicit statement. Decide deterministic-injection vs a finalize-field per all-deterministic-in-binary. Plus a SimWorld case that a strike emits the note. Plan: tasks/agent-doc/plan-queue-strike-explanation.md (Phase 1 + Phase 3 SimWorld).
- [ ] [#s81q] [recommended] #agent-doc-bug #stopvscodemenu Extend #restartagentmenu (JB Restart Agent; plan tasks/agent-doc/plan-restart-agent.md) per operator request to also add a Stop Agent command and mirror BOTH Stop+Restart into the VSCode extension menus. Delta beyond #restartagentmenu: (1) StopAgentAction.kt + plugin.xml registration on the JB side; (2) VSCode command-palette + editor/title menu contributions for stop+restart wired to the same TerminalUtil/route paths. Decide alongside-vs-replace the existing Restart Supervisor Process action; do not remove without operator confirmation. Bump pluginVersion + build signed/unsigned zips (gated by STOP-cutting-releases). Confirm scope before implementing.
- [ ] [#k7mf] [operator-verify] #g9d7verify #agent-doc-bug Live editor-less repro of #g9d7/#6b5hprimary (agent-doc submodule 260d61b3). Code shipped+pushed: the run_ipc no-delivery bail routes content_ours to the guarded controller-host disk write when there is no plugin-owner lease (live_editor_endpoint_attached=false), reusing decide_editorless_disk_fallback; a live editor still fails closed. After a make install, run agent-doc finalize in a Claude Code pane with NO JB IDE attached and confirm ops.log shows run_ipc_editorless_disk_fallback transport=disk_fallback editor_endpoint=absent plus a committed cycle, with no --force-disk and no recovery=retry_without_disk_write. Operator-gated: needs a real CLI pane.
- [ ] [#tb4q] [operator-verify] #qfocsupverify #agent-doc-bug Live-verify #qfocsup supervisor clear-and-continue for [focused-cycle] heads (agent-doc submodule 67829f3f; committed, make check green, NOT installed per STOP-cutting-releases). After make install + agent-doc admin recycle: with a [focused-cycle] head at the queue head and no in-session-drainable work, confirm the supervisor idle-watch force-clears the session (/clear) and re-dispatches the head to a fresh session instead of going idle. Proof: session-check ui_outcome=deferred_for_supervisor_drain on the yielding turn, then ops.log context_reset + go_drain_dispatch + a fresh agent draining the head.
- [ ] [#qcm1verify] [operator-verify] #agent-doc-bug Install + live-verify the #qnodemerge1 component-scoped merge (agent-doc submodule 52458f47). Code shipped+pushed, make check green (4853). Operator-gated remainder (mid-session install causes finalize-client-newer drift; STOP-cutting-releases gate): at a supervisor-recycle boundary run make install (cargo install --path . + cargo build --release + agent-doc lib-install), agent-doc admin recycle, then drive one edit-during-finalize cycle (type into a queue item while the agent writes an exchange response) and confirm ZERO cross-component splice + no console-output-in-queue corruption.
- [ ] [#1jec] [operator-verify] #yzerverify #agent-doc-bug Live-verify the #yzer/#evmhplugin plugin reconnect re-read (agent-doc 0.34.28 / submodule f6b8ae17, plugin 0.2.172). Code shipped+pushed: binary FFI agent_doc_reconnect_buffer_decision (disk==HEAD via ffi_show_head + buffer==prior-commit via ffi_show_prior_blobs) + pure decide_reconnect_buffer + PatchWatcher.registerRoot reconcile-on-connect (applyReconnectReread) + SimWorld coverage; make check green (4832). Operator-gated remainder (install is unsafe mid-session = finalize-client-newer drift): at a supervisor-recycle boundary run make install (cargo install --path . + cargo build --release + agent-doc lib-install), reinstall plugin 0.2.172, agent-doc admin recycle. Then repro: open a session doc in JB, kill/restart the supervisor so the binary commits while the plugin is disconnected, and on reconnect confirm ops.log shows 'reconnect_buffer_decision decision=reread_disk #yzer' and the editor buffer == HEAD with NO File Cache Conflict; a buffer holding genuine unsaved user edits must instead log decision=keep_buffer and be preserved (editor wins).
<!-- /agent:backlog -->

## Review
 
<!-- agent:review -->
<!-- /agent:review -->

<!-- agent:icebox -->
<!-- /agent:icebox -->

## Completed / Reaped

<!-- agent:done archive=tasks/agent-doc-bugs2.done.md -->

<!-- completed work archived in tasks/agent-doc-bugs2.done.md -->
<!-- /agent:done -->
