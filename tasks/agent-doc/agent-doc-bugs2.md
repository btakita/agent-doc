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
queue: start
---

## Status

<!-- agent:status patch=replace -->

done
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7a9b103bd5b68223dac05a4c449ac03899aba53d6022e82100578b5b0-20260627-202333.md`*

Compacted content:
- Archived 34 response topic(s): do #provauth1 #provauth2 #provauth3; drain #actorprune; drain #qheadstrikeauto; 31 more
- Prior summary/context: ❯ *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7a9b103bd5b68223dac...
- I also tried local clean `make tmux-ci`; it failed because the live test saw this active Codex pane `%0` as `alive-busy` and refused to clear it, so that local live sweep is not a clean signal in this interactive tmux session.
- `src/agent-doc/specs/06-config.md`, `08-session-routing.md`, `08b-single-process-control-plane.md`, `12-deterministic-simulation.md`, and `process-topology.md` describe the prevention surfaces: append idempotence, editor-IPC queue writes, single-writer topology, and SimWorld coverage.
- `src/agent-doc/specs/07-closeout-commands.md` records the closeout state-machine expectations for cancel recovery and accepted-conflict replay repair.
- `src/agent-doc/runbooks/jb-cache-conflict.md` is the main runbook: Accept applies through the Document API; Cancel must fail closed or auto-recover through preflight/write-commit when the response already reached the working tree; Compact Exchange must use component editor IPC when a listener is active; late Accept can replay stale IPC and duplicate a response until the planned fix revalidates against HEAD.
- Latest GitHub Actions CI run for `201270a4` is `28301694825`; repeated public Actions checks through 2026-06-27 17:20 America/New_York still showed `Status In progress`. `gh` cannot currently poll authenticated logs because its stored token is invalid and the unauthenticated API hit rate/permission limits.
- `git push origin main` passed.
- Clean committed worktree `make install-full` passed and installed `agent-doc`/`libagent_doc`.
- Clean committed worktree `make check` passed: clippy, 5,480 nextest tests, doctests, medium simulation, and Lean build.
- `cargo test -p agent-doc-orchestration run_queue_maintenance_restrikes_snapshot_struck_live_reemit` passed.
- `cargo test -p agent-doc-orchestration queue::tests::converge` passed.
- `cargo test -p agent-doc-orchestration stale_live_reemit` passed.
- `agent-doc-orchestration/src/preflight/maintenance.rs`: added a maintenance regression for the `#qeditdup` / `#qftloss#qftloss` shape where a struck snapshot line reappears live during the preflight/writeback window.
- `agent-doc-orchestration/src/queue.rs`: queue convergence now treats a struck snapshot/current representative for the same queue-head identity as authoritative over a stale live re-emit, so the unstruck copy is dropped or re-struck instead of resurrected.

### Re: IPC proof diagnostic (interrupted-cycle recovery) — agent-doc

```text
**IPC proof issue dogfood log**
Appended automatically during interrupted-cycle recovery to record the editor IPC issue.
This is binary-authored diagnostic content, not a user prompt, so it does not require a separate response cycle.
Issue class: `ipc_proof_insufficient`
Affected component: editor IPC / writeback

ipc_proof_insufficient file=/home/brian/work/btakita/agent-loop/tasks/agent-doc/agent-doc-bugs2.md source=socket_ack_content patch_id=2b631eb9-320c-4beb-a958-a5c6606d955c invariant=live_prompt_drift_after_preflight recovery=content_ours_snapshot_next_cycle snap_source=ack_content_sidecar candidate_len=60535 candidate_hash=d439f983a1af78f6bf53f4b138479447d3cc35459f79a2e20ed11d3b6aee36a1 content_ours_len=59263 content_ours_hash=5115e8a8ad9960d748139056313dcd6161dde54800dcbc12c3bd40585bd59a4e doc=agent-doc-bugs2 session=b26b9957-8de6-4eae-bb98-5d7fe4d6b781 turn=cycle-1782591710165
```

### Re: do #qeditdup and #qftloss/#qftlossproof — gpt-5

I treated `[#qftloss#qftloss]` as the legacy `#qftloss` class, whose current active replacement is `#qftlossproof`.

`#qeditdup` failed live verification. The current working tree/live-buffer resurrected a queue row that HEAD had already struck: `do [#qeditdup] [#qftloss#qftloss]` came back unstruck during the interrupted preflight/writeback window. The evidence is the latest JetBrains live-buffer sidecar `.agent-doc/live-buffer/f6d9af4a09fc7ed8.jetbrains-52613-d265e059-af8c-421f-a813-9de0ac53521c`, plus ops.log markers for `cycle-1782592777227` (`ipc_proof_insufficient`, `live_prompt_drift_after_preflight`, `preflight_blocked_uncommitted_closeout_drift`, and editor ops). I added `#qeditdupguard` for the deeper editor-edit-during-cycle guard and marked `#qeditdup` done as a failed verification superseded by that focused follow-up.

For `#qftlossproof`, the prompt text was preserved in the live-buffer sidecar and current disk state, so I did not find a free-text loss in this cycle. I am leaving `#qftlossproof` open because I did not find the full durable `qdurcrash`/replay proof needed to close that replacement item end-to-end.

No source files changed and no tests were needed. `gh run list --workflow CI --limit 1` returned GitHub 404 for `btakita/agent-loop`, so I could not inspect the latest CI run.

### Re: #qeditdupguard - gpt-5

Implemented `#qeditdupguard` in `src/agent-doc` and pushed commit `201270a4` (`fix: prevent struck queue head resurrection`) to `origin/main`.

What changed:
- `agent-doc-orchestration/src/queue.rs`: queue convergence now treats a struck snapshot/current representative for the same queue-head identity as authoritative over a stale live re-emit, so the unstruck copy is dropped or re-struck instead of resurrected.
- `agent-doc-orchestration/src/preflight/maintenance.rs`: added a maintenance regression for the `#qeditdup` / `#qftloss#qftloss` shape where a struck snapshot line reappears live during the preflight/writeback window.

Verification:
- `cargo test -p agent-doc-orchestration stale_live_reemit` passed.
- `cargo test -p agent-doc-orchestration queue::tests::converge` passed.
- `cargo test -p agent-doc-orchestration run_queue_maintenance_restrikes_snapshot_struck_live_reemit` passed.
❯ - Clean committed worktree `make check` passed: clippy, 5,480 nextest tests, doctests, medium simulation, and Lean build.
❯ - Clean committed worktree `make install-full` passed and installed `agent-doc`/`libagent_doc`.
- `git push origin main` passed.
- Latest GitHub Actions CI run for `201270a4` is `28301694825`; repeated public Actions checks through 2026-06-27 17:20 America/New_York still showed `Status In progress`. `gh` cannot currently poll authenticated logs because its stored token is invalid and the unauthenticated API hit rate/permission limits.
- I also tried local clean `make tmux-ci`; it failed because the live test saw this active Codex pane `%0` as `alive-busy` and refused to clear it, so that local live sweep is not a clean signal in this interactive tmux session.

Confidence: high for the requested queue resurrection regression; remote CI conclusion was still pending when this closeout was written.

### Re: JB File Cache Conflict occurrences — gpt-5

> **Queue prompt:** :pushpin: JB `File Cache Conflict` occurrences

I scanned the active task document, backlog/review components, archived done list, and the agent-doc runbooks/specs for `File Cache Conflict`, `jbcc*`, and cache-conflict references.

Current active occurrence: `tasks/agent-doc/agent-doc-bugs2.md` has this free-text queue head, plus active backlog `#kj5flive`, which asks for live JB+VSCode attach/detach and File Cache Conflict / `#6b5h` repro logs after the latest plugin update. I did not find a separate active backlog item that is only named `JB File Cache Conflict`; `#kj5flive` is the live-proof carrier that still mentions it.

Current source-of-truth docs/code contracts:
- `src/agent-doc/runbooks/jb-cache-conflict.md` is the main runbook: Accept applies through the Document API; Cancel must fail closed or auto-recover through preflight/write-commit when the response already reached the working tree; Compact Exchange must use component editor IPC when a listener is active; late Accept can replay stale IPC and duplicate a response until the planned fix revalidates against HEAD.
- `src/agent-doc/specs/07-closeout-commands.md` records the closeout state-machine expectations for cancel recovery and accepted-conflict replay repair.
- `src/agent-doc/specs/06-config.md`, `08-session-routing.md`, `08b-single-process-control-plane.md`, `12-deterministic-simulation.md`, and `process-topology.md` describe the prevention surfaces: append idempotence, editor-IPC queue writes, single-writer topology, and SimWorld coverage.

Historical archived occurrence clusters in `tasks/agent-doc-bugs2.done.md` are: initial Compact Exchange repro/isolation (`#jbccache`, `#jbccfix`, `#jbccreg`, `#jbccdocs`); cancel-branch implementation and proof trail (`#jbccc1`/`#jbccc3`/`#jbccc4`/`#jbccc5`, `#jbcclivecancelresid`, `#0ep7`); late-accept duplicate replay (`#jbccacceptdup`, `#wkbs`); response-outside-exchange corruption (`#jb-cache-cancel-response-outside-exchange`); syntax/highlighting loss after disk load (`#7qfw`, `#jb-file-cache-conflict-syntax-loss`); degraded IPC avoiding raw disk writes (`#ipc-degraded-prefers-file-ipc`); reconnect stale-buffer recovery (`#yzer`, `#1jec`); remaining-trigger instrumentation (`#advr19`); and realtime cutover work removing the post-commit reconcile path that used to feed these dialogs (`#pcwcrt`).

No source files changed for this queue item. Verification was `agent_doc_read` for queue/backlog/review plus a targeted `rg` scan over `tasks/agent-doc/agent-doc-bugs2.md`, `tasks/agent-doc-bugs2.done.md`, `src/agent-doc/runbooks`, `src/agent-doc/specs`, and mirrored skill runbooks.

### Re: JB File Cache Conflict occurrences — gpt-5

> **Queue prompt:** ~~:pushpin: JB `File Cache Conflict` occurrences~~ — auto-struck: answered this cycle (#ftstrike)

The `JB File Cache Conflict` queue head is answered and already struck. I verified the active document components and a targeted repo scan for `File Cache Conflict`, `jbcc*`, `cache-conflict`, and `jb-cache-conflict` references.

Current active carrier: `#kj5flive`, which is still operator-verify work for live JB+VSCode attach/detach and File Cache Conflict / `#6b5h` repro logs after the latest plugin update. I did not find a separate active backlog item solely named `JB File Cache Conflict`.

The current source-of-truth surfaces are `src/agent-doc/runbooks/jb-cache-conflict.md`, `src/agent-doc/specs/07-closeout-commands.md`, `src/agent-doc/specs/06-config.md`, `src/agent-doc/specs/08-session-routing.md`, `src/agent-doc/specs/08b-single-process-control-plane.md`, `src/agent-doc/specs/12-deterministic-simulation.md`, and `src/agent-doc/specs/process-topology.md`. Historical occurrence clusters remain archived in `tasks/agent-doc-bugs2.done.md` under the `#jbcc*`, cancel-recovery, late-accept replay, syntax-loss, degraded-IPC, reconnect-staleness, trigger-instrumentation, and realtime-cutover entries.

No source files changed for this queue item; verification was `agent_doc_read` for queue/exchange/backlog/review plus the targeted `rg` scan.

### Re: Queue residue repair — gpt-5

The answered `JB File Cache Conflict` free-text queue row is now fully struck, so the queue parser no longer treats it as active residue. `session-check` advanced the next queue head to `#syncbarrier`; this repair only touched the session-document queue/closeout state and did not change source files.

### Re: Queue repair closeout — gpt-5

Closed the queue-residue repair after the answered `JB File Cache Conflict` row was fully struck. The queue now correctly presents `#syncbarrier` as the next active head; no source files changed in this repair.
<!-- agent:boundary:32f1c9a4 -->
<!-- /agent:exchange -->
## Queue


<!-- agent:queue priority preset="#spec-test-build-install-commit-push" go -->
~~~done
:pushpin: JB `Run Agent Doc` on monsterrodholders.md
```text
[route] target tmux session: 0
Error: route refusing to dispatch agent-doc /home/brian/work/btakita/agent-loop/src/boost-client/tasks/monsterrodholders.md into pane %1 for /home/brian/work/btakita/agent-loop/src/boost-client/tasks/monsterrodholders.md because the composer contains protected prompt input (drafted prompt input) draft_preview="› Summarize recent commits"; clear or submit that draft, then rerun agent-doc route snapshot_path=/home/brian/work/btakita/agent-loop/src/boost-client/.agent-doc/logs/route-submit/1782586439553-direct_pane_protected_prompt_input-codex-_1-a0c7b4c8e461.txt
```
~~~
~~~done
:pushpin: JB `Run Agent Doc`
```
[route] target tmux session: 0
Error: route refusing to dispatch agent-doc /home/brian/work/btakita/agent-loop/tasks/agent-doc/agent-doc-bugs2.md into pane %12 for /home/brian/work/btakita/agent-loop/tasks/agent-doc/agent-doc-bugs2.md because the composer contains protected prompt input (drafted prompt input) draft_preview="› Use /skills to list available skills"; clear or submit that draft, then rerun agent-doc route snapshot_path=/home/brian/work/btakita/agent-loop/.agent-doc/logs/route-submit/1782591358742-direct_pane_protected_prompt_input-codex-_12-39e728d2278f.txt
```
~~~
---
🚧 :pushpin: monsterrodholders.md issue
```
• The failed agent-doc write inserted only a few summary bullets instead of a clean Re: #osm-tile-key-provision
  section. I’m going to fix the task doc with a narrow patch: remove my partial bullets from the compacted summary and
  add a proper exchange entry before the boundary.
```
---
- ~~:pushpin: do [#qeditdup] [#qftloss#qftloss]~~
- ~~~~:pushpin: JB `File Cache Conflict` occurrences~~ — auto-struck: answered this cycle (#ftstrike)~~
- :pushpin: do [#syncbarrier]
- :pushpin: do [#qheadstrikeauto]
- :pushpin: do [#suprestassoc]
- :pushpin: do [#2y8j]
- :pushpin: do [#cdtcutover]
- :pushpin: do [#crdtauth6]
- :pushpin: do [#nsfz]
- :pushpin: do [#kj5flive]
- :pushpin: do [#tmuxsynccrashlog]
- :pushpin: do [#qftlossproof]
- :pushpin: do [#midturnrecycleproof]
- :pushpin: do [#midturnresumeimpl]
- :pushpin: do [#provauth3bimpl]
- :pushpin: do [#supstaledetectverify]
- :pushpin: do [#suptmuxstaleverify]
- :pushpin: do [#tmuxsynccrashlog]
- :pushpin: do [#qftlossproof]
- :pushpin: do [#midturnrecycleproof]
- :pushpin: do [#midturnresumeimpl]
- :pushpin: do [#provauth3bimpl]
- :pushpin: do [#supstaledetectverify]
- :pushpin: do [#suptmuxstaleverify]
- :pushpin: do [#60ev]
- ~~do [#qeditdupguard]~~
<!-- /agent:queue -->
<!--
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
- [ ] [#tmuxsynccrashlog] Replacement for #tmuxsynccrash: JB Sync Tmux Layout live repro log review. After the operator runs repeated JB Sync Tmux Layout against the repro layout, inspect idea.log, agent-doc ops.log, and tmux server pid/session evidence; close only if the server pid stays stable with repair_layout no-op/passive supersede markers, otherwise file a follow-up bug with the failing log markers. Plan: tasks/agent-doc/plan-sync-tmux-layout-crash.md
- [ ] [#qftlossproof] Replacement for #qftloss: free-text prompt preservation live proof. After the operator types an unsaved queue/free-text prompt during concurrent finalize, inspect .agent-doc/live-buffer journal evidence plus ops.log qdurcrash/live-buffer replay markers; close only if the prompt survives end-to-end, otherwise file a focused loss bug with artifact paths. Plan: tasks/agent-doc/plan-queue-edit-race-consolidated.md
- [ ] [#midturnrecycleproof] Replacement for #midturn-recycle-resume: Phase A recycle interlock live proof. Drive a recycle in the preflight->finalize window, inspect ops.log cycle-open recycle-defer/commit markers and editor writeback proof; close only if no live_prompt_drift_after_preflight appears, otherwise file a regression. Plan: tasks/agent-doc/plan-midturn-recycle-resume-wedge.md
- [ ] [#midturnresumeimpl] Replacement for #midturnresumeb: Phase B durable turn checkpoint resume. Implement and verify consumption of the #durablerecycle turn checkpoint on fresh supervisor boot, with ops-log markers proving interrupted-child redispatch versus surviving-child no-op and deferred-too-long escalation coverage. Plan: tasks/agent-doc/plan-midturn-recycle-resume-wedge.md
- [ ] [#provauth3bimpl] Replacement for #provauth3b: operator-origin authority cutover. Once #crdtauth6 supplies live origin deltas, replace remaining content_ours/stale-editor resurrection heuristics with origin checks and add logs/tests proving operator-origin working-tree edits are never reset. Plan: tasks/agent-doc/plan-operator-provenance-authority.md
- [ ] [#supstaledetectverify] Replacement for #supstaledetect: stale-install warning merge/install verification. Merge the source-mtime warning fix, install/recycle, then verify logs show a fresh build no longer warns while a genuinely stale binary still emits stale_install evidence.
- [ ] [#suptmuxstaleverify] Replacement for #suptmuxstale: stale-supervisor pane indicator install/log proof. Merge/install the pane-title marker change, recycle, then verify .agent-doc/supervisor-stale plus pane title/turn_status logs show STALE SUPERVISOR only when the supervisor binary is actually stale.
- [ ] [#syncbarrier] #agent-doc-feature Phase 1 (editor-sync barrier) — epoch handshake so the binary DEFERS dangerous ops (snapshot reset / disk reconcile / merge-base rebuild) while an editor edit is in flight, bounded wait, fail-OPEN to the editor buffer (never a held lock, never snapshot-discard). Poll-check = yrs StateVector exchange, not text compare. Needs JB/VSCode edit_epoch + StateVector over IPC (extend State method). Phase 0 #ipctruncrecover shipped (0.34.53). Plan: tasks/agent-doc/plan-editor-sync-barrier.md
- [ ] [#qheadstrikeauto] #agent-doc-feature auto-strike answered free-text head WITHOUT requiring the agent to quote it as a > **Queue prompt:** blockquote — operator: "the binary should do this automatically...not the agent". Today strike_answered_free_text_queue_heads (queue_consume.rs) only fires on a blockquote/prose-prefix match, so a head answered in plain prose is never struck. Key the strike off the binary drain-target / in-progress (the marker) head identity for the cycle instead of agent prose formatting. Plan: tasks/agent-doc/plan-freetext-queue-head-autostrike.md
- [ ] [#suprestassoc] #agent-doc-bug restart-supervisor re-associates the wrong doc: restarting the equityfundingsource.md supervisor re-bound EFS to the agent-doc-bugs2.md claude session. Actor re-register/lease-replace likely keys on pane/session not (document_id,generation) across the shared project controller. Needs live repro; fix = scope re-association to the restarted doc only + regression. Plan: tasks/agent-doc/plan-restart-supervisor-doc-reassociation.md
- [ ] [#60ev] plan-dynamic-context-injection-ledger.md
- [ ] [#2y8j] [operator-verify] #qbasehashmemo live typing proof: after the supervisor auto-installs 0.34.49 and recycles, type a sustained burst in a large doc (equityfundingsource.md or agent-doc-bugs2.md) in the JB editor and confirm typing is smooth; grep ops.log to confirm crdt_merge_base no longer fires per keystroke during a typing burst.
- [ ] [#cdtcutover] [focused-cycle] #agent-doc-feature FUTURE/SEPARATE rung — wire the evented CellDocTree model into the live finalize/write path (replace whole-tree reconcile at finalize with update_to per-node deltas; expose SemTree queries to preflight/supervisor drainable computations). Large, live-supervisor-critical, gated, own focused cycle + operator live proof. Plan: tasks/agent-doc/plan-celldoctree-evented-reflection.md
- [ ] [#crdtauth6] [operator-verify] crdtauth5 live hookup: thin JB DocumentListener calling CrdtReplicaForwarder.forwardLocalDelta + a Kotlin Unix-socket ReplicaTransport client (writes NDJSON register/deregister/update, delivers inbound update_b64 to applyRemoteUpdate); plus the deferred VS Code koffi replica seam (editors/vscode/src/native.ts out-param marshaling). Rust fan-out + Kotlin seam already merged + tested. Plan: tasks/agent-doc/plan-crdt-authority-model.md
- [ ] [#nsfz] [operator-verify] #startupdeadline live dogfood: drive a real harness that never reaches a dispatch-ready prompt (e.g. auth-walled/hung TUI) and confirm the session fails closed within 30s with a 'session did not become dispatch-ready in Ns' stderr diagnostic + a recorded startup_miss marker, instead of hanging. Code-complete + tested (make check 5101 green, release build green); needs a live editor/pane eyeball.
- [ ] [#kj5flive] [operator-verify] #mergestatemachine3 live proof replacement for cleared review #kj5f: after the latest JB plugin update is installed, verify JB+VSCode attach/detach/ownership facts flow through the merge-ownership state machine via #lazilystatesync, then run live editor-attached/detached repros for File Cache Conflict and the #6b5h wedge with logs sufficient to prove/disprove the fix before filing follow-up bugs. Plan: tasks/agent-doc/plan-merge-control-state-machine.md
<!-- /agent:backlog -->

## Review
 
<!-- agent:review -->
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
