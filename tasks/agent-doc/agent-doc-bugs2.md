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
  '#spec-test-build-install-commit-push': red-green-refactor. update spec + tests. build + install for local testing. commit + push
  '#spec-test-commit-push': red-green-refactor. update spec + tests. commit + push
queue: start
---

## Status

<!-- agent:status patch=replace -->

🔴 (restart/recycle your supervisor)
done
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7a9b103bd5b68223dac05a4c449ac03899aba53d6022e82100578b5b0-20260630-235719.md`*

Compacted content:
- Archived 3 response topic(s): continue; do [#qftlossproof]; do [#qftlossproof]
- Prior summary/context: prior compacted content: Archived 20 response topic(s): IPC proof diagnostic (interrupted-cycle recovery); IPC proof ...
- `make install` — installed `agent-doc` and `libagent_doc.so`; installer marked 2 controllers to recycle at the next idle boundary.
- `make check` — clippy, 6155 nextest tests, doctests, SimWorld medium seed, and Lean all passed.
- `cargo test -p agent-doc-queue in_progress_marker_detection_allows_priority_prefix`
- `cargo test -p agent-doc-orchestration abandoned`
- Refactor: updated the cycle-state contract docs to distinguish abandoned stale preflight terminal state from a later real commit closeout that supersedes it.
- Green: `cargo test -p agent-doc-orchestration abandoned` passes, including the new terminal-proof regression and the cycle-state unit regression.
- Red: the new closeout regression failed before the fix with `terminal proof cannot record closeout ... is abandoned`.
`/clear` prompt items are added to the agent without me running JB `Clear Session Context`. There seems to be a background or retry task doing this. Please continue to simplify.

JB `Run Agent Doc` error
```
[route] target tmux session: 0
Error: route refusing to dispatch agent-doc /home/brian/work/btakita/agent-loop/tasks/agent-doc/agent-doc-bugs2.md into pane %67 for /home/brian/work/btakita/agent-loop/tasks/agent-doc/agent-doc-bugs2.md because the composer contains protected prompt input (drafted prompt input) draft_preview="› Summarize recent commits"; clear or submit that draft, then rerun agent-doc route snapshot_path=/home/brian/work/btakita/agent-loop/.agent-doc/logs/route-submit/1782863907349-direct_pane_protected_prompt_input-codex-_67-3390d64c34ee.txt
```

#spec-test-build-install-commit-push

### Re: preflight exchange precedence — gpt-5

You were right. The bad selection had two separate causes: preflight let an active queue head stay effective while an exchange prompt was waiting, and the continuous backlog refill path treated `queue: start` as if it were an explicit `go` opt-in even when the `agent:queue` tag had no `go` attribute.

I fixed both in `src/agent-doc` and pushed commit `05cc1380` (`fix: prioritize exchange prompt over queue preflight`). Preflight now captures an unresolved exchange prompt before its commit/boundary-reposition step can hide it, synthesizes that as the effective exchange diff, clears effective selected queue prompts for the turn, and drops `queue_continuation_required` while the exchange prompt is pending. Queue refill/same-cycle pending-add sync now require explicit `go` (`queue: go` or marker-side `go`); plain `queue: start` remains durable active state only.

Verification: focused regressions passed, `make check` passed, `cargo install --path .` installed `agent-doc 0.34.65`, and the session supervisor is marked to recycle at the next idle boundary. The pushed CI run `28484451473` was still in progress at closeout. No JB plugin update is needed for this fix.
❯ Fix
```text
**IPC proof issue dogfood log**
Appended automatically during interrupted-cycle recovery to record the editor IPC issue.
This is binary-authored diagnostic content, not a user prompt, so it does not require a separate response cycle.
Issue class: `ipc_proof_insufficient`
Affected component: editor IPC / writeback

ipc_proof_insufficient file=/home/brian/work/btakita/agent-loop/tasks/recruit/reitrades.md source=socket_ack_content patch_id=- invariant=missing_response_probe recovery=retry_without_disk_write response_sha256=8e43288335a8442441841ad3a6316187ead230a5a33f3c8b672314ac30f56d68 content_len=27112 content_hash=9d611d65347b8874f8d48343639874c4e6174037efa37f2e6f3dc8d6199fb467 doc=reitrades session=62d195b8-b029-42a5-9c6c-836cb1b5b103 turn=cycle-1782863860494
```

❯ Archived 3 response topic(s): continue; do [#qftlossproof]; do [#qftlossproof]
### Session Summary


- `make install` — installed `agent-doc` and `libagent_doc.so`; installer marked 2 controllers to recycle at the next idle boundary.
- `make check` — clippy, 6155 nextest tests, doctests, SimWorld medium seed, and Lean all passed.
- `cargo test -p agent-doc-queue in_progress_marker_detection_allows_priority_prefix`
- `cargo test -p agent-doc-orchestration abandoned`
- Refactor: updated the cycle-state contract docs to distinguish abandoned stale preflight terminal state from a later real commit closeout that supersedes it.
- Green: `cargo test -p agent-doc-orchestration abandoned` passes, including the new terminal-proof regression and the cycle-state unit regression.
- Red: the new closeout regression failed before the fix with `terminal proof cannot record closeout ... is abandoned`.

```
[route] target tmux session: 0
Error: route refusing to dispatch agent-doc /home/brian/work/btakita/agent-loop/tasks/agent-doc/agent-doc-bugs2.md into pane %67 for /home/brian/work/btakita/agent-loop/tasks/agent-doc/agent-doc-bugs2.md because the composer contains protected prompt input (drafted prompt input) draft_preview="› Summarize recent commits"; clear or submit that draft, then rerun agent-doc route snapshot_path=/home/brian/work/btakita/agent-loop/.agent-doc/logs/route-submit/1782863907349-direct_pane_protected_prompt_input-codex-_67-3390d64c34ee.txt
```


### Re: preflight exchange precedence — gpt-5

You were right. The bad selection had two separate causes: preflight let an active queue head stay effective while an exchange prompt was waiting, and the continuous backlog refill path treated `queue: start` as if it were an explicit `go` opt-in even when the `agent:queue` tag had no `go` attribute.

I fixed both in `src/agent-doc` and pushed commit `05cc1380` (`fix: prioritize exchange prompt over queue preflight`). Preflight now captures an unresolved exchange prompt before its commit/boundary-reposition step can hide it, synthesizes that as the effective exchange diff, clears effective selected queue prompts for the turn, and drops `queue_continuation_required` while the exchange prompt is pending. Queue refill/same-cycle pending-add sync now require explicit `go` (`queue: go` or marker-side `go`); plain `queue: start` remains durable active state only.

Verification: focused regressions passed, `make check` passed, `cargo install --path .` installed `agent-doc 0.34.65`, and the session supervisor is marked to recycle at the next idle boundary. The pushed CI run `28484451473` was still in progress at closeout. No JB plugin update is needed for this fix.

### Re: terminal-proof abandoned preflight closeout — gpt-5

Fixed and pushed in `src/agent-doc` as `fee78f7a` (`fix: close terminal proof after abandoned preflight`).

The root cause was `cycle_state::mark_committed`: after the supervisor marked an interrupted/stale preflight as `abandoned`, a later successful closeout commit called `mark_committed`, but the transition table quite correctly refused `Abandoned -> Committed`. `mark_committed` then returned the stale abandoned state unchanged, so strict terminal proof failed later with `cycle ... is abandoned`.

I kept terminal proof strict. The fix is that a real commit closeout now supersedes an abandoned stale preflight with a fresh synthetic `write_applied -> committed` cycle, while already-committed cycles stay idempotent and lower-rank abandoned bookkeeping still stays terminal.

Red-green-refactor evidence:

- Red: the new closeout regression failed before the fix with `terminal proof cannot record closeout ... is abandoned`.
- Green: `cargo test -p agent-doc-orchestration abandoned` passes, including the new terminal-proof regression and the cycle-state unit regression.
- Refactor: updated the cycle-state contract docs to distinguish abandoned stale preflight terminal state from a later real commit closeout that supersedes it.

Verification completed:

- `cargo test -p agent-doc-orchestration abandoned`
- `cargo test -p agent-doc-queue in_progress_marker_detection_allows_priority_prefix`
- `make check` — clippy, 6155 nextest tests, doctests, SimWorld medium seed, and Lean all passed.
- `make install` — installed `agent-doc` and `libagent_doc.so`; installer marked 2 controllers to recycle at the next idle boundary.

GitHub CI for `fee78f7a` is currently in progress: run `28485233701`.
❯ Fix
```text
**IPC proof issue dogfood log**
Appended automatically during interrupted-cycle recovery to record the editor IPC issue.
This is binary-authored diagnostic content, not a user prompt, so it does not require a separate response cycle.
Issue class: `ipc_proof_insufficient`
Affected component: editor IPC / writeback

ipc_proof_insufficient file=/home/brian/work/btakita/agent-loop/tasks/recruit/reitrades.md source=socket_ack_content patch_id=- invariant=missing_response_probe recovery=retry_without_disk_write response_sha256=8e43288335a8442441841ad3a6316187ead230a5a33f3c8b672314ac30f56d68 content_len=27112 content_hash=9d611d65347b8874f8d48343639874c4e6174037efa37f2e6f3dc8d6199fb467 doc=reitrades session=62d195b8-b029-42a5-9c6c-836cb1b5b103 turn=cycle-1782863860494
```

❯ Archived 3 response topic(s): continue; do [#qftlossproof]; do [#qftlossproof]
<!-- patch:exchange -->
### Re: archived qftlossproof prompts — gpt-5

Grouped response for `continue; do [#qftlossproof]; do [#qftlossproof]`: I’m treating this as a compaction/archive state signal, not a fresh request to re-run `#qftlossproof`.

Both `#qftlossproof` queue entries are already struck, and the unresolved follow-up is represented by `#qftlossdelta` at the top of the backlog. No additional `#qftlossproof` dispatch is needed from this archived marker.

<!-- /patch:exchange -->

### Re: do [#provauth3bimpl] — gpt-5

I audited the operator-origin authority invariant under the hard rule from `tasks/agent-doc/plan-operator-provenance-authority.md`.

No source patch was needed in this cycle: the remaining unsafe `content_ours` / stale-editor resurrection class is already covered by the current operator-preserving and fail-closed guards. The existing regressions assert that operator-visible document text is authoritative, that fallback paths must preserve user edits through `agent-doc write --stream`, and that docs/instructions must not reintroduce `content_ours`, stale snapshots, or ACK-content as authority for dropping operator text.

Changed paths for this queue item: none. I left the unrelated dirty `src/agent-doc` worktree changes untouched.

Verification passed:
- `cargo test -p agent-doc --test test_cli root_agents_instructions_preserve_operator_authority`
- `cargo test -p agent-doc --test test_cli docs_do_not_reintroduce_content_ours_snapshot_authority`
- `cargo test -p agent-doc-orchestration operator_text_authority`
- `cargo test -p agent-doc-orchestration content_ours`
- `cargo test -p agent-doc-orchestration live_buffer`
- `cargo test -p agent-doc-orchestration visible_write`
- `cargo test -p agent-doc-orchestration stale_supervisor_content_ours`
- parent repo: `33b314d2d` (`Update agent-doc submodule for operator buffer preservation`)
- `src/agent-doc`: `61566d04` (`Preserve live editor digests over write provenance`)
- Latest GitHub Actions CI was `in_progress` for workflow `CI`, run `28493126754`, at 2026-07-01T04:16:45Z.
- `make check` completed with exit code 0 via tsift digest runner; the digest runner misclassified two `PASS ... editor_route_error` lines as failures, but the command succeeded.
- `cargo test -p agent-doc-debounce` passed: 50 tests.
- `cargo fmt -p agent-doc-debounce --check`

Confidence: high for the audit result and regression coverage. Escalation: none; I did not use `--force-disk`.

### Re: :pushpin: do [#supstaledetectverify] — gpt-5

> **Queue prompt:**
>
> :pushpin: do [#supstaledetectverify]

`agent-doc` is installed: `agent-doc 0.34.65`.

I could not safely finalize `#supstaledetectverify`. The response is captured, but closeout is blocked by `editor_convergence_required`: the JetBrains live buffer for `tasks/agent-doc/agent-doc-bugs2.md` is unsaved/ahead of disk, and current plugin code intentionally disables `save_document` IPC. I retried `finalize`/`write --commit`, refreshed the live buffer, recycled the supervisor, and did not use `--force-disk`.

Next safe operator action: save or resolve the live JetBrains buffer for `tasks/agent-doc/agent-doc-bugs2.md`, then rerun:

```bash
agent-doc write --commit tasks/agent-doc/agent-doc-bugs2.md
```

Only use `--force-disk` if you explicitly want to override the live-editor safety guard.

### Re: IPC proof diagnostic (interrupted-cycle recovery) — agent-doc

```text
**IPC proof issue dogfood log**
Appended automatically during interrupted-cycle recovery to record the editor IPC issue.
This is binary-authored diagnostic content, not a user prompt, so it does not require a separate response cycle.
Issue class: `ipc_proof_insufficient`
Affected component: editor IPC / writeback

ipc_proof_insufficient file=tasks/agent-doc/agent-doc-bugs2.md source=file_ipc patch_id=0c4fde76-ae00-4e2e-b9ec-ea2d2569924f invariant=no_ack recovery=retry_without_disk_write timeout_secs=2 patch_file=/home/brian/work/btakita/agent-loop/.agent-doc/patches/483039e7a9b103bd5b68223dac05a4c449ac03899aba53d6022e82100578b5b0.json doc=agent-doc-bugs2 session=b26b9957-8de6-4eae-bb98-5d7fe4d6b781 turn=cycle-1782881096099
```

### Re: :pushpin: do [#suptmuxstaleverify] — gpt-5

> **Queue prompt:**
> :pushpin: do [#suptmuxstaleverify]

Completed the stale-supervisor pane-indicator verification pass.

Evidence:
- Focused tests passed: `cargo test -p agent-doc-turn turn_status`, `cargo test -p agent-doc-document stale_supervisor`, `cargo test -p agent-doc-orchestration supervisor_stale_marker`, `cargo test -p agent-doc-orchestration fresh_supervisor_clears_stale_status_marker`, and `cargo test -p agent-doc --test test_cli test_agent_doc_document_owns_status_projection_policy`.
- `make install` completed successfully and installed `agent-doc 0.34.65`.
- Live stale proof: supervisor PID `3121460` maps `/home/brian/.cargo/bin/agent-doc (deleted)` at inode `95814823`, mtime `1782880713`, size `39651008`, while the installed binary is inode `95814825`, mtime `1782882042`, size `39676744`.
- With that actually-stale supervisor, `.agent-doc/supervisor-stale` is present and tmux pane `%69` has title `⚠ STALE SUPERVISOR`.
- `agent-doc admin recycle tasks/agent-doc/agent-doc-bugs2.md` accepted restart receipt `5092` with `background_started=true`; the same stale PID remains only because this pane is still busy with an open turn, so the live fresh-transition must wait for the commit/idle boundary.
- Fresh-side behavior is covered by the pure/maintenance tests: fresh title generation omits the warning, and preflight maintenance removes the stale status marker when the live supervisor is not stale.

Changed source paths: none in this verification cycle; the pane-title marker implementation was already in the installed source tree.

Confidence: high that the warning appears only for a genuinely stale supervisor. Escalation: none; I did not use `--force-disk` or manually kill the live supervisor.

### Re: external termination recovery for :pushpin: do [#suptmuxstaleverify] — gpt-5

> **Queue prompt:**
> :pushpin: do [#suptmuxstaleverify]

I investigated the externally terminated run and completed the interrupted `#suptmuxstaleverify` closeout.

What happened:
- The interrupted cycle was `cycle-1782882001811`. It repeatedly reached `response_captured` between 2026-07-01T05:02:15Z and 2026-07-01T05:06:06Z, but never reached `committed` because the convergence gate stayed unmet: `committed,editor_converged`.
- The supervisor was stale and auto-install/recycle ran during that stuck cycle. At 2026-07-01T05:07:40Z the cycle was marked `abandoned` with `event=suprecyclespin_stalled_cycle_resolved`, then the stale supervisor hot-reloaded onto the freshly installed binary.
- A later recovery cycle, `cycle-1782882659553`, successfully committed the `#suptmuxstaleverify` response as commit `fd24c070d`. That response records the stale-supervisor proof, focused test evidence, successful `make install`, and the fact that the live fresh transition waits for idle after commit.
- A follow-up preflight found another stale `preflight_started` cycle, `cycle-1782882671249`, appended the IPC proof diagnostic for the already-applied socket response, and committed that repair as `2b93bd940`. The remaining issue was live-buffer exchange drift beyond the committed snapshot, which this finalize closes through the normal binary-owned path.

No source files needed changes in this recovery pass. The prior verification evidence for `#suptmuxstaleverify` remains the authoritative completion proof, and this closeout preserves the diagnostic text instead of manually rewriting the session document.
<!-- agent:boundary:c15949ba -->
<!-- /agent:exchange -->
## Queue


<!-- agent:queue priority preset="#spec-test-build-install-commit-push" -->
- ~~:pushpin: do [#syncbarrier]~~
- ~~:pushpin: do [#qheadstrikeauto]~~
- ~~:pushpin: do [#suprestassoc]~~
- :pushpin: do [#2y8j]
- :pushpin: do [#cdtcutover]
- :pushpin: do [#crdtauth6]
- :pushpin: do [#nsfz]
- :pushpin: do [#kj5flive]
- :pushpin: do [#tmuxsynccrashlog]
- ~~:pushpin: do [#qftlossproof]~~
- ~~:pushpin: do [#midturnrecycleproof]~~
- ~~:pushpin: do [#midturnresumeimpl]~~
- ~~:pushpin: do [#provauth3bimpl]~~
- ~~:pushpin: do [#supstaledetectverify]~~
- ~~:pushpin: do [#suptmuxstaleverify]~~
- :pushpin: do [#tmuxsynccrashlog]
- ~~:pushpin: do [#qftlossproof]~~
- ~~:pushpin: do [#provauth3bimpl]~~
- ~~:pushpin: do [#suptmuxstaleverify]~~
- 🚧 :pushpin: do [#60ev]
- :pushpin: do [#suprestassoc-live]
- :pushpin: do [#agentsignals]
- :pushpin: do [#qftlossdelta]
- :pushpin: do [#coredecomp]
- :pushpin: do [#orchdecomp]
- :pushpin: do [#skillharnessplugin]
- :pushpin: do [#elementgraphprd]
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
- [ ] [#kjw0] JetBrains Run Agent Doc route/dispatch failed after bounded submit/start proof retries #jbrunautobug #agent-doc-bug failure_class=accepted_without_dispatch_start_proof document=tasks/agent-doc/agent-doc-bugs2.md stage=dispatch_start_proof pane=%69 actor_generation=1161 editor_attempt_id=1782871961019-10 dispatch_proof_state=none elapsed_ms=21174 diagnostic_path=/home/brian/work/btakita/agent-loop/.agent-doc/logs/route-submit/1782872116210-direct_pane_dispatch_start_unproven-codex-_69-a2246a0e8c6f.txt ops_log_path=/home/brian/work/btakita/agent-loop/.agent-doc/logs/ops.log ops_log_marker=route_submit_issue(issue=accepted_without_dispatch_start_proof,phase=dispatch_start_proof,result=accepted_without_dispatch_start_proof) [symptom-key invariant=run_agent_doc_route_dispatch_failure document=483039e7a9b103bd5b68223dac05a4c449ac03899aba53d6022e82100578b5b0 component=route/dispatch_start_proof content_hash=sha256:15e835a6e0f770b814aa68069d2a7354628b2401ea485f84d2a929ae58cc6554]
- [ ] [#restartstderrbleed] #agent-doc-bug Diagnose binary-restart stderr bleed into the agent after supervisor/PCP restart; capture the stderr source and add regression/proof that restart diagnostics stay out of the agent prompt/UI. Related JB Run Agent Doc route failure remains tracked by #kjw0.
- [ ] [#qftlossdelta] #agent-doc-bug focused invariant-loss follow-up from failed #qftlossproof live proof: `.agent-doc/logs/ops.log:4338964-4338997` shows ACK/live-buffer content at len 16676 hash `83f08ab7e6be49a671f96f7c0ffbb71fe28bba858929b883c5904140cabd5192`, then closeout committed len 16669 hash `dc278d260b5b2a3efffeb12c428f0f81f5be1c8373fdd3ef044eee4b39adda4f`; `live_buffer_classify` suppressed the differing operator-capable buffer with `reason=write_provenance_newer_than_buffer` and no durable sidecar preserving the 16676-byte content was found. Fix the operator-wins merge/write-provenance path so concurrent operator text is preserved end-to-end or the write fails closed with a recoverable artifact. Plan: tasks/agent-doc/plan-queue-edit-race-consolidated.md
- [ ] [#tmuxsynccrashlog] [operator-verify] Replacement for #tmuxsynccrash: JB Sync Tmux Layout live repro log review. After the operator runs repeated JB Sync Tmux Layout against the repro layout, inspect idea.log, agent-doc ops.log, and tmux server pid/session evidence; close only if the server pid stays stable with repair_layout no-op/passive supersede markers, otherwise file a follow-up bug with the failing log markers. Plan: tasks/agent-doc/plan-sync-tmux-layout-crash.md
- [ ] 🚧 [#60ev] plan-dynamic-context-injection-ledger.md
- [ ] [#2y8j] [operator-verify] #qbasehashmemo live typing proof: after the supervisor auto-installs 0.34.49 and recycles, type a sustained burst in a large doc (equityfundingsource.md or agent-doc-bugs2.md) in the JB editor and confirm typing is smooth; grep ops.log to confirm crdt_merge_base no longer fires per keystroke during a typing burst.
- [ ] [#cdtcutover] [focused-cycle] #agent-doc-feature FUTURE/SEPARATE rung — wire the evented CellDocTree model into the live finalize/write path (replace whole-tree reconcile at finalize with update_to per-node deltas; expose SemTree queries to preflight/supervisor drainable computations). Large, live-supervisor-critical, gated, own focused cycle + operator live proof. Plan: tasks/agent-doc/plan-celldoctree-evented-reflection.md
- [ ] [#coredecomp] [focused-cycle] Keep the former `agent-doc-core` God-crate surface decomposed into owned pure crates. Audit `pending.rs`, `queue_item_lifecycle.rs`, `template.rs`, `crdt_sync.rs`, `component.rs`, and related modules; move element-local semantics into `agent-doc-element-*`, document composition into `agent-doc-document`, merge/CRDT merge policy into `agent-doc-merge`, and FFI ABI into `agent-doc-ffi`. Do not recreate `agent-doc-core` as a compatibility facade or migration alias, and do not move runtime IO, snapshots, commits, IPC, tmux, or controller effects into pure crates.
- [ ] [#orchdecomp] [focused-cycle] Decompose the `agent-doc-orchestration` God crate along effect boundaries. Audit turn lifecycle, document realtime scheduling, queue/exchange lifecycle, closeout/commit, editor IPC, supervisor/controller state, tmux integration, recovery/repair, and session routing; move stable responsibilities into `agent-doc-turn`, `agent-doc-document`/document-realtime, `agent-doc-supervisor`/controller-state crates, `agent-doc-editor`, `agent-doc-tmux`, and narrow closeout/recovery crates as appropriate. Leave `agent-doc-orchestration` as a shrinking effect adapter only, not a deprecated compatibility facade; callers should move directly to the owning focused crate and pure merge/document code should stay out of orchestration.
- [ ] [#skillharnessplugin] [focused-cycle] Create the agent-doc integration plugin for `skill-harness`: implement an agent-doc-owned `SkillHarnessPlugin` adapter, define its detection and skill-path contract without depending on `agent-doc-core`, register/document the integration path, and add tests proving `skill-harness` can resolve agent-doc through the plugin boundary after `agent-doc-core` removal.
- [ ] [#crdtauth6] [operator-verify] CRDT authority live proof: verify the existing JB DocumentListener -> CrdtReplicaForwarder.forwardLocalDelta path, Kotlin Unix-socket ReplicaTransport, VS Code koffi replica seam, and Rust fan-out stay connected in a live editor session. This is proof/parity work only; operator-visible text is already authoritative and must not wait on this item. Plan: tasks/agent-doc/plan-crdt-authority-model.md
- [ ] [#nsfz] [operator-verify] #startupdeadline live dogfood: drive a real harness that never reaches a dispatch-ready prompt (e.g. auth-walled/hung TUI) and confirm the session fails closed within 30s with a 'session did not become dispatch-ready in Ns' stderr diagnostic + a recorded startup_miss marker, instead of hanging. Code-complete + tested (make check 5101 green, release build green); needs a live editor/pane eyeball.
- [ ] [#kj5flive] [operator-verify] #mergestatemachine3 live proof replacement for cleared review #kj5f: after the latest JB plugin update is installed, verify JB+VSCode attach/detach/ownership facts flow through the merge-ownership state machine via #lazilystatesync, then run live editor-attached/detached repros for File Cache Conflict and the #6b5h wedge with logs sufficient to prove/disprove the fix before filing follow-up bugs. Plan: tasks/agent-doc/plan-merge-control-state-machine.md
- [ ] [#suprestassoc-live] [operator-verify] Restart the `tasks/professional/equityfundingsource.md` supervisor in a live editor/tmux setup and verify the actor/admin projection keeps EFS bound to its own pane/session without re-associating to `tasks/agent-doc/agent-doc-bugs2.md`.
- [ ] [#agentsignals] [future/separate] Add a realtime agent:signals element after core realtime convergence is stable. Start with deterministic built-in signals for tests, queue, supervisor, and ipc; design custom user-defined signals as plugin-backed tracked work with Rust as the first target and future multi-language support; use tasks/software/realtime-signals-patchboard.md as the GUI/semi-programmatic reference; keep MLP enhancement optional/experimental and not load-bearing for deterministic states.
- [ ] [#elementgraphprd] [future/separate] Define the single-document element layout for both single-model sessions and multi-harness orchestration, including typed graph/workers/jobs/schedule elements plus an advisory perceptron graph that scores visible signals without directly mutating runnable work. PRD: tasks/agent-doc/prd-element-layout-perceptron-graph.md
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
- [ ] [#exttasksources] #agent-doc-feature Add external task source adapters that project PM-tool tasks into the active document/work graph as source-backed nodes, keeping local queue/closeout authority document-owned and making remote push-back explicit and audited. PRD: tasks/agent-doc/prd-external-task-sources.md
<!-- /agent:icebox -->

## Completed / Reaped

<!-- agent:done archive=tasks/agent-doc-bugs2.done.md -->

<!-- completed work archived in tasks/agent-doc-bugs2.done.md -->
<!-- /agent:done -->
