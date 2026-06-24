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
  '#advance-review': Go through the review items. Add actionable backlog + queue items to complete or advance the review items.,
  '#agent-doc-bug': Please create a plan for agent-doc to fix this issue. Add to the backlog of tasks/agent-doc/agent-doc-bugs2.md
  '#code-review': Please review agent-doc codebase. Inspect the architecture and look for edge cases. Isolate concurrency issues and other workflow bugs. Let's find root causes to fix.
  '#commit-push': commit + push all uncommitted files
  '#follow-up-backlog': Any follow-up items to place in the backlog?
  '#mermaid': Include explanatory mermaid diagram(s).
  '#next-steps': Any follow-up items to place in the backlog? Please ensure the backlog is in priority order. Print the item id of the top of the backlog.
  '#simworld': Strengthen Simworld tests.
  '#spec-test-build-install-commit-push': update spec + tests. build + install for local testing. commit + push
  '#spec-test-commit-push': update spec + tests. commit + push
queue: stop
---

## Status

<!-- agent:status patch=replace -->

done
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7a9b103bd5b68223dac05a4c449ac03899aba53d6022e82100578b5b0-20260624-004520.md`*

Compacted content:
- Archived 17 response topic(s): #mbd4 IPC issue dogfooding log; #p6a0 deferred session clear dedupe; :pushpin: do [#pzjy]; 14 more
- Prior summary/context: prior compacted content: Archived 10 response topic(s): ack-mismatch shorter recovered response loss; #directchatpb2;...
- Codex dispatch-start tracking is enabled when the session document can see the workspace `.codex/hooks.json`, including nested agent-doc roots.
- Direct-pane delivery logs pane-input acceptance separately from harness start proof: `direct_pane_submit` covers the tmux text+Enter acceptance window, while `dispatch_start_proof` covers Codex hook advance or OpenCode pane-state proof.
- If a tracked dispatch leaves the input surface but no dispatch-start proof arrives, route logs `route_submit_observation result=accepted_without_dispatch_start_proof`, emits `route_submit_issue issue=accepted_without_dispatch_start_proof`, preserves a redacted pane snapshot, returns `RoutedDispatchStartProof::DispatchStartUnproven`, and the dispatch-only gate fails closed.
- Untracked accepted-only delivery still logs `proof=accepted proof_scope=accepted_only`, which preserves the distinction for log review without claiming dispatch-start proof.
- Changed paths: no source files changed in `src/agent-doc`; the pre-existing dirty files `agent-doc-orchestration/src/write.rs` and `agent-doc-orchestration/src/write/queue_consume.rs` were left untouched.
- Commands: `tsift status`; `cargo test -p agent-doc-orchestration route_submit_observation -- --nocapture`; `cargo test -p agent-doc-orchestration dispatch_only_tracked_timeout_fails_closed_even_when_accepted_only_is_allowed -- --nocapture`; `cargo test -p agent-doc-orchestration codex_dispatch_start_tracking_enabled -- --nocapture`; `gh run list --workflow CI --limit 1`; `agent-doc reset --from-current /home/brian/work/btakita/agent-loop/tasks/agent-doc/agent-doc-bugs2.md`.
- Verification: all three focused test commands passed. Latest `src/agent-doc` CI was green: run `28066398449`, `fix: file route dispatch proof failures`, completed 2026-06-24T00:24:34Z.
- Confidence: high for the diagnostic/audit result.
- Escalation: none.
**IPC proof issue dogfood log**

Detected during interrupted-cycle recovery. This note is appended automatically so the session document records editor IPC issues while queue edits are being dogfooded.

Issue class: `ipc_proof_insufficient`
Affected component: editor IPC / writeback

```text
ipc_proof_insufficient file=/home/brian/work/btakita/agent-loop/tasks/agent-doc/agent-doc-bugs2.md source=socket_ack_content patch_id=64b2305b-0126-40fe-ae97-c23fd2834c06 invariant=live_prompt_drift_after_preflight recovery=content_ours_snapshot_next_cycle snap_source=ack_content_sidecar candidate_len=39743 candidate_hash=e7a3fcd5884ac70ff46056c80578fc940bcf336609b789c279517bad549335d8 content_ours_len=40450 content_ours_hash=c946686fe9fb6aa877e1a2620bfa7daf257aab4082a54ca94574cea15b6ad889 doc=agent-doc-bugs2 session=b26b9957-8de6-4eae-bb98-5d7fe4d6b781 turn=cycle-1782259426340
```
**IPC proof issue dogfood log**

Detected during interrupted-cycle recovery. This note is appended automatically so the session document records editor IPC issues while queue edits are being dogfooded.

Issue class: `ipc_proof_insufficient`
Affected component: editor IPC / writeback


### Re: IPC proof dogfood note — gpt-5

The current prompt-bearing change is the automatic IPC proof dogfood note for `ipc_proof_insufficient` on `agent-doc-bugs2.md`. I am treating it as telemetry for the editor IPC/writeback path, not as a new source-code request in this cycle.

I already verified `#t0d0`: current route diagnostics separate pane-input acceptance from dispatch-start proof, and tracked accepted-without-proof dispatches log `route_submit_issue issue=accepted_without_dispatch_start_proof` and fail closed. No source patch was needed.

Closeout notes:
- Changed paths: session document state only; no `src/agent-doc` source files changed.
- Commands: `agent-doc reset --from-current /home/brian/work/btakita/agent-loop/tasks/agent-doc/agent-doc-bugs2.md`; `agent-doc repair --apply-recovery /home/brian/work/btakita/agent-loop/tasks/agent-doc/agent-doc-bugs2.md`; focused route tests listed in the previous audit note.
- Verification: prior focused route tests passed, and latest `src/agent-doc` CI was green (`28066398449`).
- Confidence: medium for the closeout recovery because the editor IPC sidecar repeatedly reported stale snapshot drift; high for the `#t0d0` route diagnostic audit.
- Escalation: none.

No new follow-up backlog item is needed from this IPC telemetry note. `#advance-review` remains queued; this cycle did not drain it.
```text
ipc_proof_insufficient file=/home/brian/work/btakita/agent-loop/tasks/agent-doc/agent-doc-bugs2.md source=socket_ack_content patch_id=9d04336f-6af2-4a7e-89d6-62d63a895517 invariant=live_prompt_drift_after_preflight recovery=content_ours_snapshot_next_cycle snap_source=ack_content_sidecar candidate_len=20046 candidate_hash=0814c3876aeabcad6d7be9f17261101e985e2edd144c8f0fc26b4d025e580df4 content_ours_len=50021 content_ours_hash=44851fb2ef72b930cbaa8e613adb11d3e9f642bfb568fa0cc598b477fc7d09ac doc=agent-doc-bugs2 session=b26b9957-8de6-4eae-bb98-5d7fe4d6b781 turn=cycle-1782263068189
```
Completed the agent-doc cycle.

> **Queue prompt:**
>
> #advance-review


`#t0d0` is closed as an audit: current `src/agent-doc` already separates pane-input acceptance from dispatch/start proof, logs `accepted_without_dispatch_start_proof`, and fails closed for tracked unproven dispatches. No source patch was needed.

Verification passed:
`route_submit_observation`, `dispatch_only_tracked_timeout_fails_closed_even_when_accepted_only_is_allowed`, and `codex_dispatch_start_tracking_enabled`. Latest `src/agent-doc` CI was green: `28066398449`.

Session document closeout is committed as `3f6d5c32a`, and `agent-doc session-check` is OK. `#advance-review` remains queued, but there is no drainable continuation right now. Note: `tasks/agent-doc-bugs2.done.md` remains locally modified; it was already dirty in the worktree and I did not commit it.
<!-- agent:boundary:d38e5ae4 -->
<!-- /agent:exchange -->

## Queue

<!-- agent:queue priority preset="#spec-test-build-install-commit-push" priority go -->
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
<!-- /agent:backlog -->

## Review
 
<!-- agent:review -->
- [/] [#tb4q] [operator-verify] #qfocsupverify #agent-doc-bug Live-verify #qfocsup supervisor clear-and-continue for [focused-cycle] heads (agent-doc submodule 67829f3f; committed, make check green, NOT installed per STOP-cutting-releases). After make install + agent-doc admin recycle: with a [focused-cycle] head at the queue head and no in-session-drainable work, confirm the supervisor idle-watch force-clears the session (/clear) and re-dispatches the head to a fresh session instead of going idle. Proof: session-check ui_outcome=deferred_for_supervisor_drain on the yielding turn, then ops.log context_reset + go_drain_dispatch + a fresh agent draining the head.
- [/] [#qnodemerge4verify] [operator-verify] #agent-doc-bug #qnodemerge4wire Phase 4 live non-ASCII verification (agent-doc submodule 4650054c; base-hash FFI + JB 0.2.173 + VS Code 0.2.29 reporters committed+pushed, make check + JB gradle + tsc/tests green, cdylib lib-installed). REMAINING operator-gated: (1) install the JB plugin zip from disk (build/distributions/agent-doc-jetbrains-0.2.173-signed.zip) + restart IDEA, and install the VS Code .vsix (editors/vscode/agent-doc-0.2.29.vsix) + reload window — the Kotlin/TS reporter changes need a plugin reinstall, the cdylib FFI is already hot-reloaded. (2) In a live editor, type a NON-ASCII edit (e.g. insert/delete around "café 日本 😀") into a session doc while an agent merge runs; confirm ops.log shows editor_op_recorded kind=insert/delete with BYTE offsets (not UTF-16) and that the concurrent merge aligns to the real edit boundary (no character interleaving / mojibake). The base-hash gate + replay==theirs gate make a misaligned op fall back to the diff-guess safely, so a wrong offset cannot corrupt — verification confirms the ops are ACCEPTED (editor_ops_for_base match), not merely safe.
- [/] [#fbwireverify] [operator-verify] #agent-doc-bug #fbwireverify Live two-pane make tmux-ci acceptance for #fbwire/#fullboundary Phase 2 (agent-doc submodule 85b173fb; committed+pushed, make check green 4956, clippy clean; NOT installed per STOP-cutting-releases). After make install (cargo install --path . + cargo build --release + agent-doc lib-install) + agent-doc admin recycle at a recycle boundary, drive two live panes: (1) a slow-ACK editor drain must never pile IPC inflight > 1 — confirm ops.log shows convergence_gate_defer lines (unmet=inflight_drained) before each dispatch and NO inflight>1; (2) a wedged (no-ACK) editor must trip the bounded CONVERGENCE_GATE_TIMEOUT_MS (30s) → ERROR-level convergence_gate_force_disk_fallback severity=error + a loadable .agent-doc/playback/<hash>/<cycle>.json artifact, and the drain proceeds (never permanently stuck). Operator-gated: needs two live editor/CLI panes the driving session cannot host.
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
- [ ] [#qloop1] #agent-doc-feature Define durable queue loop annotation syntax and parser/rendering behavior: mark loop-only queue/backlog entries, keep annotation/state out of prompt completion proof, and expose a stable current-item marker for editor/supervisor surfaces.
- [ ] [#qloop2] #agent-doc-feature Implement idle-loop scheduling for annotated loop items: when normal drainable queue work is empty, dispatch loop items in deterministic sequence, persist the loop cursor/last-run state, and prove regular queue items always preempt loop work.
- [ ] [#qloop3] #agent-doc-feature Add loop poll controls: global default poll period plus per-loop override, next-run time gates, bounded backoff/jitter, and tests proving no tight spin when there are no relevant events.
- [ ] [#qloop4] #agent-doc-feature Add event-aware loop scheduling: record deterministic event counters/timestamps and let loop items schedule earlier/later based on event frequency before any model-assisted scheduling hook.
- [ ] [#qobs1] #agent-doc-feature Add a bounded observer telemetry snapshot surface: supervisor/session actors expose compact per-document state (cycle phase, queue head, actor readiness, IPC inflight, recent typed ops-log markers, and artifact handles) without dumping raw transcripts by default.
- [ ] [#qobs2] #agent-doc-feature Implement deterministic observer rules that scan session-check/preflight/ops-log snapshots for annotated issues and recurring failure patterns, then file deduped session-doc backlog bugs with evidence paths and stable issue fingerprints.
- [ ] [#qobs3] #agent-doc-feature Add a sidecar observer agent-doc loop that can monitor all running project sessions from snapshot artifacts, rate-limit bug filing, avoid pane interference, and route created bug items to the correct session document or a configured observer inbox.
- [ ] [#qobs4] #agent-doc-feature Add model-assisted observer triage as advisory-only: feed compact snapshot/context-pack handles to a subscription or local model, require deterministic evidence before auto-filing bugs, and record model provenance/confidence on suggestions.
- [ ] [#qlm1] #agent-doc-feature Define queue item backend-selection syntax for local-model execution, separate from queued `/model <tier>` switches: support explicit per-item backend/model annotations on normal and loop queue items without changing prompt text or completion proof.
- [ ] [#qlm2] #agent-doc-feature Implement a local-model queue dispatcher backend that can run eligible queue/loop items through a configured local model, capture stdout/structured results, and write responses through the normal agent-doc closeout boundary.
- [ ] [#qlm3] #agent-doc-feature Add local-model dispatch safety gates and telemetry: capability checks, GPU/resource lease integration, fallback policy, provenance logs, and tests proving local-model queue items cannot be silently marked complete without a committed response/proof.
<!-- /agent:icebox -->

## Completed / Reaped

<!-- agent:done archive=tasks/agent-doc-bugs2.done.md -->

<!-- completed work archived in tasks/agent-doc-bugs2.done.md -->
<!-- /agent:done -->
