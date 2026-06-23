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

complete
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

❯ *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7a9b103bd5b68223dac05a4c449ac03899aba53d6022e82100578b5b0-20260623-040841.md`*

Compacted content:
❯ - Archived 5 response topic(s): #ws55; #ws55; #3x90; 2 more

### Re: dispatch-ready actor proof — gpt-5

> **Queue prompt:** Extending the timeout is not the solution. The panes were up is less than 2s. Codex was running and idle. I am able to use Codex manually. This is an agent-doc dispatch bug.

Corrected. The 120s JetBrains wait bump was not the root fix for this report; it remains only as slow-start margin. The actual dispatch bug was the dispatch-only startup gate treating a stale open `codex_start` log as authoritative even when the same session/pane already had controller-backed ready proof.

Changed paths:
- `src/agent-doc/agent-doc-orchestration/src/route/dispatch_only.rs`
- `src/agent-doc/VERSIONS.md`
- parent submodule pointer `src/agent-doc`

What changed:
- Added a dispatch-only actor-ready proof path before and after the existing short pane-capture wait. It loads the authoritative actor binding for the same session/pane and bypasses the stale startup-log wait only when the actor is eligible, the effective actor state is `Ready`, and the existing prompt-ready barrier is satisfied.
- Kept fail-closed behavior for busy, starting, degraded/no-socket, missing, or wrong-pane actors. Those still fall through to the existing live probe/recovery path.
- Logged the accepted proof as `route_dispatch_only_starting_pane_ready_via_actor_state ... prompt_ready=true` so this class is auditable next time.
- Updated `VERSIONS.md` to say the timeout was not the root fix and to describe the stale-startup-log/ready-actor dispatch fix.

Commits:
- `3a61ce19` in `src/agent-doc`: `fix dispatch ready actor proof` (pushed to `btakita/agent-doc` `main`)
- `cf1bdc73f` in the parent repo: `agent-doc: update dispatch ready fix`
- Prior timeout-margin commit remains: `5dfb210f` (`fix jetbrains run startup wait`)

Commands run:
- `cargo fmt`
- `cargo test -p agent-doc-orchestration dispatch_only_starting_pane_actor_ready_gate_requires_same_ready_prompt_proven_actor`
- `cargo test -p agent-doc-orchestration route::dispatch_only::tests`
- `make check` (first run exposed the FlowCore `reason=` budget; I changed the new log field from `transition_reason=` to `transition=` and reran)
- `make tmux-ci`
- `git push origin main` in `src/agent-doc`
- `make install` in `src/agent-doc`
- `gh run list --workflow CI --limit 1`

Verification:
- Focused regression: passed.
- Dispatch-only test slice: 15 passed, 1 ignored live tmux integration.
- `make check`: passed; 5005 nextest tests passed, 241 skipped; doctests passed; wider deterministic closeout sim passed.
- `make tmux-ci`: passed; 7 ignored/live tests passed.
- Local install: `make install` rebuilt `/home/brian/.cargo/bin/agent-doc` and `libagent_doc.so`; it also marked 1 controller for auto-recycle at the next idle boundary.
- GitHub Actions CI run `28002487677` for `3a61ce19` was still `in_progress` at the last check, not red.

Confidence: high for the reported failure class. The fix directly addresses the operator correction: an already-idle, controller-proven-ready Codex actor is no longer blocked by a stale startup-log probe.

Escalation: none needed. Remaining live note: the currently serving route-owned supervisor still reported stale until it reaches the idle boundary and processes the auto-recycle marker from `make install`.
<!-- agent:boundary:acc9dddf -->
<!-- /agent:exchange -->

## Queue

<!-- agent:queue priority preset="#spec-test-build-install-commit-push" priority go -->
I rebooted. JB `Run Agent Doc` stalled without dispatching. Prompt has `/clear` from another watcher. `/clear` should not have been added.
```
resync: pruned 3 stale session(s)
[sync] recovered live pane %6 for session b26b9957 via path provenance pid=179547 instance=f14e0d0d-a615-46dc-9372-095c6f262ee8
[sync] recovered live pane %3 for session 62fe1f41 via supervisor pid 140998
resync: purged 1 orphaned stash pane(s)
[route] target tmux session: 0
[route] activated existing inactive agent:queue head "do [#fcccap]" for tasks/agent-doc/agent-doc-bugs2.md (already_present=true, activated=true) before dispatch-only reopen
[route] Still waiting for codex (1s)... last line:   gpt-5.5 xhigh · ~/work/btakita/agent-loop · Context 0% use
[route] Still waiting for codex (3s)... last line:   gpt-5.5 xhigh · ~/work/btakita/agent-loop · Context 0% use
[route] Still waiting for codex (4s)... last line:   gpt-5.5 xhigh · ~/work/btakita/agent-loop · Context 0% use
[route] Still waiting for codex (6s)... last line:   gpt-5.5 xhigh · ~/work/btakita/agent-loop · Context 0% use
[route] Still waiting for codex (7s)... last line:   gpt-5.5 xhigh · ~/work/btakita/agent-loop · Context 0% use
[route] Still waiting for codex (9s)... last line:   gpt-5.5 xhigh · ~/work/btakita/agent-loop · Context 0% use
[route] Still waiting for codex (10s)... last line:   gpt-5.5 xhigh · ~/work/btakita/agent-loop · Context 0% use
[route] Still waiting for codex (12s)... last line:   gpt-5.5 xhigh · ~/work/btakita/agent-loop · Context 0% use
[route] Still waiting for codex (14s)... last line:   gpt-5.5 xhigh · ~/work/btakita/agent-loop · Context 0% use
[route] Still waiting for codex (15s)... last line:   gpt-5.5 xhigh · ~/work/btakita/agent-loop · Context 0% use
[route] Still waiting for codex (17s)... last line:   gpt-5.5 xhigh · ~/work/btakita/agent-loop · Context 0% use
[route] Still waiting for codex (18s)... last line:   gpt-5.5 xhigh · ~/work/btakita/agent-loop · Context 0% use
[route] Still waiting for codex (20s)... last line:   gpt-5.5 xhigh · ~/work/btakita/agent-loop · Context 0% use
[route] Still waiting for codex (21s)... last line:   gpt-5.5 xhigh · ~/work/btakita/agent-loop · Context 0% use
[route] Still waiting for codex (23s)... last line:   gpt-5.5 xhigh · ~/work/btakita/agent-loop · Context 0% use
[route] Still waiting for codex (24s)... last line:   gpt-5.5 xhigh · ~/work/btakita/agent-loop · Context 0% use
[route] Still waiting for codex (26s)... last line:   gpt-5.5 xhigh · ~/work/btakita/agent-loop · Context 0% use
[route] Still waiting for codex (27s)... last line:   gpt-5.5 xhigh · ~/work/btakita/agent-loop · Context 0% use
[route] Still waiting for codex (29s)... last line:   gpt-5.5 xhigh · ~/work/btakita/agent-loop · Context 0% use
[route] Still waiting for codex (30s)... last line:   gpt-5.5 xhigh · ~/work/btakita/agent-loop · Context 0% use
[route] Still waiting for codex (32s)... last line:   gpt-5.5 xhigh · ~/work/btakita/agent-loop · Context 0% use
[route] Still waiting for codex (33s)... last line:   gpt-5.5 xhigh · ~/work/btakita/agent-loop · Context 0% use
[route] Still waiting for codex (35s)... last line:   gpt-5.5 xhigh · ~/work/btakita/agent-loop · Context 0% use
[route] Still waiting for codex (36s)... last line:   gpt-5.5 xhigh · ~/work/btakita/agent-loop · Context 0% use
[route] Still waiting for codex (38s)... last line:   gpt-5.5 xhigh · ~/work/btakita/agent-loop · Context 0% use
[route] Still waiting for codex (39s)... last line:   gpt-5.5 xhigh · ~/work/btakita/agent-loop · Context 0% use
[route] Still waiting for codex (41s)... last line:   gpt-5.5 xhigh · ~/work/btakita/agent-loop · Context 0% use
[route] Still waiting for codex (42s)... last line:   gpt-5.5 xhigh · ~/work/btakita/agent-loop · Context 0% use
[route] Still waiting for codex (44s)... last line:   gpt-5.5 xhigh · ~/work/btakita/agent-loop · Context 0% use
[route] Still waiting for codex (46s)... last line:   gpt-5.5 xhigh · ~/work/btakita/agent-loop · Context 0% use
[route] Still waiting for codex (47s)... last line:   gpt-5.5 xhigh · ~/work/btakita/agent-loop · Context 0% use
[route] Still waiting for codex (49s)... last line:   gpt-5.5 xhigh · ~/work/btakita/agent-loop · Context 0% use
[route] Still waiting for codex (50s)... last line:   gpt-5.5 xhigh · ~/work/btakita/agent-loop · Context 0% use
[route] Still waiting for codex (52s)... last line:   gpt-5.5 xhigh · ~/work/btakita/agent-loop · Context 0% use
[route] Still waiting for codex (53s)... last line:   gpt-5.5 xhigh · ~/work/btakita/agent-loop · Context 0% use
[route] Still waiting for codex (55s)... last line:   gpt-5.5 xhigh · ~/work/btakita/agent-loop · Context 0% use
[route] Still waiting for codex (56s)... last line:   gpt-5.5 xhigh · ~/work/btakita/agent-loop · Context 0% use
[route] Still waiting for codex (58s)... last line:   gpt-5.5 xhigh · ~/work/btakita/agent-loop · Context 0% use
[route] Still waiting for codex (59s)... last line:   gpt-5.5 xhigh · ~/work/btakita/agent-loop · Context 0% use
Error: dispatch-only codex reopen refused to inject into pane %6 for tasks/agent-doc/agent-doc-bugs2.md because the latest run is still booting and never reached a dispatch-ready prompt (timed_out); wait for the pane to become ready and reroute again ui_outcome_contract=ui-outcome-v1 ui_outcome=blocked_with_exact_unblocker ui_outcome_class=blocked next_action=follow_unblocker unblocker=wait_for_dispatch_ready_prompt
```
---
- ~~Extending the timeout is not the solution. The panes were up is less than 2s. Codex was running and idle. I am able to use Codex manually. This is an agent-doc dispatch bug.~~
- do [#fcccap]
- do [#fccsup]
- do [#fccendpoint]
- do [#fccforce]
- do [#fccnoack]
- do [#fccpatch]
- do [#fccowner]
- do [#fccack]
- do [#fccsave]
- do [#fccreconnect]
- do [#fccaudit2]
<!-- /agent:queue -->
<!--
- It does not seems like the JB `Run Agent Doc` submit retries are being applied correctly. Still does not submit.
- JB `Run Agent Doc` still does not submit. The submit retry poll should be 500ms and attempt for 30s...only if the input contents are the prompt itself. Other automated additions to the prompt should be forbidden. If the user modifies the agent prompt, stop the retry polling.
---
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
- [ ] [#jbrunclear] #agent-doc-bug Fix the remaining JetBrains Run Agent Doc `/clear` interference: when an editor-initiated route is pending or a recovered pane is still booting, supervisor/PCP context-reset handoff must not leave `/clear` or `/new` in the composer; add ops proof that distinguishes route-submit-in-flight, context-clear marker, prompt-ready ordering, and the 2026-06-23 reboot shape.
- [ ] [#fcccap] #agent-doc-bug Replace manual-only File Cache Conflict capture (#cyh0) with an automated trigger bundle: on every JB File Cache Conflict/pending-memory event, write a correlated artifact containing the nearest agent-doc command, ops.log `transport=... action=... reason=...`, patch id/editor id, plugin event (`pending`, `kept-memory`, `cancelled`, `saveDocument`), and document hash so the remaining trigger is provable without operator log scraping.
- [ ] [#fccsup] #agent-doc-bug Close the stale-binary supervisor FCC path: make stale route-owned supervisors recycle at the next safe boundary without requiring a manual `admin recycle`, and add a regression/ops assertion that `content_ours_adoption_refused_stale_supervisor reason=supervisor_binary_stale` cannot recur after install+boundary recycle.
- [ ] [#fccendpoint] #agent-doc-bug Harden the `apply_template_writeback transport=disk_fallback reason=send_failed editor_endpoint=absent action=editorless_disk_fallback` path: distinguish true no-editor CLI sessions from JB-open-but-missing/stale owner lease, fail closed or restore editor ownership for the latter, and add coverage so an open JB buffer is never externally overwritten as an editorless disk fallback.
- [ ] [#fccforce] #agent-doc-bug Contain `convergence_gate_force_disk_fallback reason=editor_ipc_convergence_boundary_failed`: make the fallback write require a loadable playback artifact plus editor-open safety proof, prefer blocked/error with exact unblocker when JB has the file open, and prove the force-disk path cannot surface a File Cache Conflict silently.
- [ ] [#fccnoack] #agent-doc-bug Trace and harden active-listener/no-ACK behavior: when `transport=blocked reason=no_ack_content` fires, prove no plugin/VFS side effect (`saveDocument`, VCS refresh, stale patch apply) can still raise a JB File Cache Conflict; add ops markers and a regression for the blocked path staying non-mutating.
- [ ] [#fccpatch] #agent-doc-bug Fix PatchWatcher conflict-pending flow: when IntelliJ already has a File Cache Conflict / memory-vs-disk prompt, defer patch application and deletion until the conflict resolves, then either rebase/retry the patch with fresh base proof or discard with an explicit logged reason instead of leaving the original external-write source ambiguous.
- [ ] [#fccowner] #agent-doc-bug Finish the multi-window plugin-consumer cause beyond #8bfz operator proof: make the single-owner lease fail-closed or loudly degraded for untargeted patches when the owner lease is absent/stale/FFI-unavailable, log owner/non-owner decisions with document hash and editor id, and add a live/simulated multi-window regression that only one JB process calls `saveDocument` per patch.
- [ ] [#fccack] #agent-doc-bug Prove and harden ACK-content sidecar behavior: label `.agent-doc/ack-content` writes separately from session-doc writes, assert sidecar writes themselves never target the visible markdown file, and when ACK-content is missing or stale, route to blocked/error recovery instead of any fallback that can cause an external file change under JB.
- [ ] [#fccsave] #agent-doc-bug Instrument and gate plugin `saveDocument` / VCS-refresh signals as possible FCC triggers: log source action and before/after hashes for every plugin save/refresh after patch application, suppress saves when the editor buffer is stale vs HEAD/disk, and add coverage for the stale-buffer save not reintroducing disk/editor divergence.
- [ ] [#fccreconnect] #agent-doc-bug Complete the plugin reconnect stale-buffer half of #evmh: on JB plugin reconnect, if the editor buffer is stale relative to HEAD/disk and has no proven user edit, re-read disk/HEAD instead of pushing the stale buffer via `saveDocument`; cover reconnect-after-binary-write and reconnect-after-user-edit separately.
- [ ] [#fccaudit2] #agent-doc-bug Run a remaining File Cache Conflict write-site audit: enumerate every session-document mutation path (reset/repair/preflight/status/queue/done/finalize/compact/route recovery/plugin refresh), require each to use the converge-or-disk gate or a documented editorless proof, and add a `write_authority` ops marker with a stable cause label for any path that can still touch disk.
<!-- /agent:backlog -->

## Review
 
<!-- agent:review -->
- [/] [#jjds] [operator-verify] Confirm in a live editor/pane that a stale-binary supervisor recycles at the next idle boundary and resumes queue drain on agent-doc 0.34.36.
- [/] [#arsm] [operator-verify] #pcwcwarnverify #agent-doc-bug Live-verify the #pcwcwarn post-commit stale-exchange reconcile (agent-doc submodule 5f3fc07d, superproject db7fc3594). Code shipped+pushed+make-check-green (4947). Runs in the binary commit path + editor IPC, so it goes live once the supervisor adopts the new build (admin recycle done). REMAINING (operator-gated, needs a live editor): drive one edit-during-finalize cycle with a stale > Queue prompt blockquote left inside the exchange in a live JB/VS Code buffer; confirm ops.log shows postcommit_exchange_reconciled reason=stale_editor_exchange AND zero recurring postcommit_worktree_check match=false drift on the next cycle (no manual git checkout HEAD needed).
- [/] [#cancelturnverify] [operator-verify] #agent-doc-bug Live-verify Cancel Turn (agent-doc submodule b4471d05; committed+pushed, make check green, JB 0.2.176 + VS Code 0.2.31 built, NOT installed). After installing the plugins: (1) while a turn is RUNNING, invoke Cancel Turn (JB menu / VS Code command) and confirm the turn is interrupted (ops.log cancel_turn_performed action=interrupt_active_turn) and the agent stays open; (2) while the agent is IDLE, invoke Cancel Turn (even repeatedly) and confirm it is a NO-OP (ops.log cancel_turn_noop reason=idle_no_active_turn) and the agent is NOT closed — the core safety requirement. Verify per harness: Codex/OpenCode (Escape/C-c, never raw Ctrl+C that would close on idle) and Claude.
- [/] [#tb4q] [operator-verify] #qfocsupverify #agent-doc-bug Live-verify #qfocsup supervisor clear-and-continue for [focused-cycle] heads (agent-doc submodule 67829f3f; committed, make check green, NOT installed per STOP-cutting-releases). After make install + agent-doc admin recycle: with a [focused-cycle] head at the queue head and no in-session-drainable work, confirm the supervisor idle-watch force-clears the session (/clear) and re-dispatches the head to a fresh session instead of going idle. Proof: session-check ui_outcome=deferred_for_supervisor_drain on the yielding turn, then ops.log context_reset + go_drain_dispatch + a fresh agent draining the head.
- [/] [#qnodemerge4verify] [operator-verify] #agent-doc-bug #qnodemerge4wire Phase 4 live non-ASCII verification (agent-doc submodule 4650054c; base-hash FFI + JB 0.2.173 + VS Code 0.2.29 reporters committed+pushed, make check + JB gradle + tsc/tests green, cdylib lib-installed). REMAINING operator-gated: (1) install the JB plugin zip from disk (build/distributions/agent-doc-jetbrains-0.2.173-signed.zip) + restart IDEA, and install the VS Code .vsix (editors/vscode/agent-doc-0.2.29.vsix) + reload window — the Kotlin/TS reporter changes need a plugin reinstall, the cdylib FFI is already hot-reloaded. (2) In a live editor, type a NON-ASCII edit (e.g. insert/delete around "café 日本 😀") into a session doc while an agent merge runs; confirm ops.log shows editor_op_recorded kind=insert/delete with BYTE offsets (not UTF-16) and that the concurrent merge aligns to the real edit boundary (no character interleaving / mojibake). The base-hash gate + replay==theirs gate make a misaligned op fall back to the diff-guess safely, so a wrong offset cannot corrupt — verification confirms the ops are ACCEPTED (editor_ops_for_base match), not merely safe.
- [/] [#fbwireverify] [operator-verify] #agent-doc-bug #fbwireverify Live two-pane make tmux-ci acceptance for #fbwire/#fullboundary Phase 2 (agent-doc submodule 85b173fb; committed+pushed, make check green 4956, clippy clean; NOT installed per STOP-cutting-releases). After make install (cargo install --path . + cargo build --release + agent-doc lib-install) + agent-doc admin recycle at a recycle boundary, drive two live panes: (1) a slow-ACK editor drain must never pile IPC inflight > 1 — confirm ops.log shows convergence_gate_defer lines (unmet=inflight_drained) before each dispatch and NO inflight>1; (2) a wedged (no-ACK) editor must trip the bounded CONVERGENCE_GATE_TIMEOUT_MS (30s) → ERROR-level convergence_gate_force_disk_fallback severity=error + a loadable .agent-doc/playback/<hash>/<cycle>.json artifact, and the drain proceeds (never permanently stuck). Operator-gated: needs two live editor/CLI panes the driving session cannot host.
- [/] [#26f9] [operator-verify] #clearresubmitverify #agent-doc-bug Live-verify restart/recycle recovery with a visible Codex `/clear` or OpenCode `/new` draft: agent-doc 0.34.37 / submodule b63ca59c should submit the visible clear command before re-dispatching `agent-doc tasks/agent-doc/agent-doc-bugs2.md`, with no stranded input requiring operator Enter.
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
