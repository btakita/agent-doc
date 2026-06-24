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

done
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7a9b103bd5b68223dac05a4c449ac03899aba53d6022e82100578b5b0-20260623-222916.md`*

Compacted content:
- Archived 10 response topic(s): ack-mismatch shorter recovered response loss; #directchatpb2; JB Run Agent Doc submit path; 7 more
- Prior summary/context: prior compacted content: Archived 6 response topic(s): JB Run Agent Doc submit latency; #turnstuck-readybusy; answere...
❯ - `src/agent-doc` commit `644d7b12 fix: guard queued clear dispatch`
❯ - Clean sibling worktree at `644d7b12`: `make check` passed, including clippy, 5039 nextest tests, doctests, and the ignored medium sim check.
❯ - `cargo test -p agent-doc-orchestration context_clear_marker_resubmits_visible_pending_clear_once -- --nocapture`
❯ - `cargo test -p agent-doc-orchestration drain_settle_gate_blocks_until_pane_idles_after_clear -- --nocapture`
❯ - `cargo test -p agent-doc-orchestration focused_cycle_head_drains_for_supervisor_not_in_session_loop -- --nocapture`
❯ - `cargo test -p agent-doc-orchestration supervisor_drains_focused_cycle_but_loop_defers_it -- --nocapture`
❯ - `cargo test repeated_busy_session_clear_keeps_single_deferred_clear -- --nocapture`
❯ - `cargo test busy_clear -- --nocapture`
❯ - `cargo test queue_dispatch -- --nocapture`
❯ - Added a dispatcher regression proving a queued `/clear` does not reach raw supervisor injection, and retargeted the generic supervisor-inject test to `/doctor` so it still covers non-clear command dispatch.
❯ - Non-clear queued commands still use the existing supervisor IPC path.
❯ - That reuses the current busy-pane/active-auto-queue behavior: the first non-interrupting busy clear writes the deferred handoff/cooldown, duplicates are suppressed, and raw `/clear` is not injected into an active response.
❯ - Queued `/clear` commands now leave `queue_dispatch` before generic supervisor IPC or tmux fallback and call the existing guarded `session_actor_cmd::clear` path.
❯ - `src/agent-doc/src/queue_dispatch.rs`
- Latest GitHub Actions CI for `main` was green: run `28060207399`, `fix: preserve prose queue heads`, completed 2026-06-23T22:03:57Z.
- `make install-full` passed from that isolated worktree and installed `agent-doc 0.34.43`; install marked two controllers for recycle at next idle boundary.
- Full `make check` passed in an isolated worktree containing only this patch: 5042 tests passed, 241 skipped, medium simulator passed.
- Targeted regressions passed for queue preemption, session-clear messages, and SimWorld duplicate suppression.
- `cargo fmt --check` passed in the working tree.

### Re: #mbd4 IPC issue dogfooding log — gpt-5

> **Queue prompt:** do [#mbd4]

Implemented `#mbd4` in `src/agent-doc`.

IPC proof failures now get mirrored into the session document during interrupted-cycle recovery: preflight reads the latest matching `ipc_proof_insufficient` ops-log diagnostic, appends a deduped visible IPC dogfood note inside `agent:exchange`, and commits it through the normal recovery/commit path. The note is deliberately not a markdown heading or `### Re:` block, so session-check does not treat the automatic log as a new prompt or unowned assistant response.

The write-path direct-append approach was tested and rejected during this cycle because it created post-commit working-tree drift; the landed implementation keeps the note on the recovery side of the commit boundary.

Changed paths: `src/agent-doc/agent-doc-orchestration/src/preflight.rs`
Commands: `tsift status`; `cargo test -p agent-doc-orchestration ipc_dogfood_note`; `cargo test -p agent-doc-orchestration route_queue_commit_boundary`; `cargo test -p agent-doc-orchestration ipc_proof`; `cargo test -p agent-doc-orchestration ipc_`; clean-worktree `make check` at `dad06399`
Verification: all listed commands passed except an earlier main-worktree `make check` was blocked by unrelated unstaged queue-consumption edits; the clean sibling worktree at commit `dad06399` passed `make check`.
Commit: `src/agent-doc` `dad06399 fix: log IPC proof failures in session exchange`
Confidence: high
Escalation: none.

### Re: #p6a0 deferred session clear dedupe — gpt-5

Completed `#p6a0`.

`agent-doc session clear` now treats a busy active auto-loop as a single deferred handoff: the first non-interrupting clear records one pending clear for the next proven idle boundary, and repeated clears while that marker exists report `session_clear already deferred` without refreshing cooldown or injecting another `/clear` into the active turn. The supervisor simulator now covers that duplicate suppression and the later idle-boundary delivery.

Updated the shared and editor specs, README, `VERSIONS.md`, and the installed agent-doc skill text. I committed and pushed the agent-doc submodule fix as `ef365e4b` (`fix: dedupe deferred session clears`) and committed the parent submodule pointer as `8042b8955`.

Verification:
- `cargo fmt --check` passed in the working tree.
- Targeted regressions passed for queue preemption, session-clear messages, and SimWorld duplicate suppression.
- Full `make check` passed in an isolated worktree containing only this patch: 5042 tests passed, 241 skipped, medium simulator passed.
- `make install-full` passed from that isolated worktree and installed `agent-doc 0.34.43`; install marked two controllers for recycle at next idle boundary.
- Latest GitHub Actions CI for `main` was green: run `28060207399`, `fix: preserve prose queue heads`, completed 2026-06-23T22:03:57Z.

The live worktree still has pre-existing unrelated dirty files in `agent-doc-orchestration/src/write.rs` and `agent-doc-orchestration/src/write/queue_consume.rs`; I did not stage or commit them. A direct `make check` in the dirty worktree is currently blocked by those queue-consumption edits, so I used the isolated-patch worktree for the full check.

> **Queue prompt:** JB Run Agent Doc still not submitting. Can we track when this happens and automatically file a bug so I don't have to endlessly repeat myself when you fail to fix the issue?

I did not fold that separate auto-filing behavior into `#p6a0`. I added it as a new backlog item so the route failure path can be designed with the right diagnostic capture and dedupe contract instead of being mixed into the deferred-clear fix.
Implemented and pushed `#p6a0`.

Code commits:
- `src/agent-doc`: `ef365e4b fix: dedupe deferred session clears`
- parent gitlink: `8042b8955 chore: update agent-doc submodule`
- session closeout: `76ca6e39b agent-doc(agent-doc-bugs2): 2026-06-23 18:51:43`

What changed:
- Busy `agent-doc session clear` now queues one deferred clear for the next idle boundary.
- Repeat clears while pending now report already deferred and do not inject another `/clear`.
- Added queue-preemption, session message, and SimWorld regression coverage.
- Updated README/spec/editor specs/`VERSIONS.md`.
- Added backlog item `#3ygp` for automatic bug filing on repeated JetBrains Run route failures.

Verification:
- `cargo fmt --check` passed.
- Targeted regressions passed.
- Full `make check` passed in an isolated worktree containing only this patch: 5042 passed, 241 skipped, medium sim passed.
- `make install-full` passed and installed `agent-doc 0.34.43`.
- `src/agent-doc` CI was green before push.
- Root `Agent Doc` workflow failed in 4s with empty job steps and no logs, matching an external CI-start/no-log blocker rather than a test failure.

Notes:
- I left pre-existing unrelated dirty files unstaged, including `agent-doc-orchestration/src/write.rs` and `write/queue_consume.rs`.
- `session-check` is ok; it still warns that the route-owned supervisor is stale and should recycle at the next idle boundary.

### Re: :pushpin: do [#pzjy] — gpt-5

> **Queue prompt:**
>
> :pushpin: do [#pzjy]


Implemented `#pzjy`.

- Added post-commit queue reconciliation so committed `agent:queue` completions stay completed when a stale editor buffer reappears with the same prompt active again. This covers pinned/id-backed rows and answered free-text rows, preserves unrelated new queue work, and keeps the existing editor-owned direction where a newly struck live row wins over an active HEAD row.
- Wired the reconcile into the post-commit match decision before transient replay-hash equality can hide strike-only queue drift, with editor-IPC/disk transports and FlowCore audit coverage.
- Added the missing `--force-disk` closeout path for pending-maintenance reap after this live closeout exposed that recovery gap; ordinary preflight/route maintenance still fails closed under active listeners.
- Added focused low-level, post-commit, force-disk maintenance, and FlowCore regressions, plus spec and version notes.

Verification:
- `cargo fmt --check`
- `cargo test -p agent-doc-orchestration postcommit_queue -- --nocapture`
- `cargo test -p agent-doc-orchestration postcommit_worktree_reconciles_stale_queue_unstrikes_before_flush -- --nocapture`
- `cargo test -p agent-doc-orchestration force_disk_closeout_pending_maintenance_bypasses_active_listener -- --nocapture`
- `cargo test -p agent-doc --test test_cli flowcore_hot_path_guard_and_proof_tokens_are_budgeted -- --nocapture`
- Clean isolated worktree `make check`: clippy, 5047 nextest tests, doctests, and medium simulation passed.
- Installed the verified clean build with `make install-full`.

Commits:
- `src/agent-doc` `63edca4d` (`fix: preserve completed queue heads after stale editor drift`), pushed to `origin/main`.
- `src/agent-doc` `a58eb3e8` (`fix: force pending maintenance under force-disk closeout`), pushed to `origin/main`.
- Parent gitlink `9d0c6b5c3` (`chore: update agent-doc submodule`).
- Added focused low-level and post-commit regressions, plus spec and version notes.

Verification:
- `cargo fmt --check`
- `cargo test -p agent-doc-orchestration postcommit_queue -- --nocapture`
- `cargo test -p agent-doc-orchestration postcommit_worktree_reconciles_stale_queue_unstrikes_before_flush -- --nocapture`
- `cargo test -p agent-doc --test test_cli flowcore_hot_path_guard_and_proof_tokens_are_budgeted -- --nocapture`
❯ - Clean isolated worktree `make check`: clippy, 5046 nextest tests, doctests, and medium simulation passed.
- Installed the verified clean build with `make install-full`.

Commits:
- `src/agent-doc` `63edca4d` (`fix: preserve completed queue heads after stale editor drift`), pushed to `origin/main`.
- Parent gitlink `dbed747e1` (`chore: update agent-doc submodule`).
- `src/agent-doc` `63edca4d` (`fix: preserve completed queue heads after stale editor drift`), pushed to `origin/main`.
- Installed the verified clean build with `make install-full`.
❯ - Clean isolated worktree `make check`: clippy, 5046 nextest tests, doctests, and medium simulation passed.
- `cargo test -p agent-doc --test test_cli flowcore_hot_path_guard_and_proof_tokens_are_budgeted -- --nocapture`
- `cargo test -p agent-doc-orchestration postcommit_worktree_reconciles_stale_queue_unstrikes_before_flush -- --nocapture`
- `cargo test -p agent-doc-orchestration postcommit_queue -- --nocapture`
- `cargo fmt --check`
- Added focused low-level and post-commit regressions, plus spec and version notes.
- Wired the reconcile into the post-commit match decision before transient replay-hash equality can hide strike-only queue drift, with editor-IPC/disk transports and FlowCore audit coverage.
- Added post-commit queue reconciliation so committed `agent:queue` completions stay completed when a stale editor buffer reappears with the same prompt active again. This covers pinned/id-backed rows and answered free-text rows, preserves unrelated new queue work, and keeps the existing editor-owned direction where a newly struck live row wins over an active HEAD row.

### Re: stale #pzjy replay drift — gpt-5

This cycle did not require new source work. Preflight surfaced stale editor replay/prompt-prefix drift in the just-committed `#pzjy` exchange tail, not a new actionable queue item. I’m closing that drift through the binary-owned path so the queue can continue from a clean boundary.

Verification:
- `agent-doc preflight /home/brian/work/btakita/agent-loop/tasks/agent-doc/agent-doc-bugs2.md` opened cycle `cycle-1782257556456` and identified the stale `#pzjy` exchange drift.
- No source files changed in this cleanup cycle.

### Re: queue restoration repair — gpt-5

Restored the next queue head that was accidentally marked complete during stale closeout cleanup. No source work was performed in this repair cycle.

### Re: annotated idle loop queue items — gpt-5

> **Queue prompt:** Can we have annotated queue item(s) that runs when there are no other queue items? So this would effectively be a running loop. This should also support multiple annotated loop queue items that run in a sequence loop. Also, can we have an emoji annotate which queue item is currently being run? There can be a default poll period with configurable override. Also, the loop item can intelligently schedule based on the frequency of events.

Yes, but this should be implemented as a scheduler feature rather than overloading ordinary `agent:queue` drain semantics. The safe shape is: normal drainable queue work always wins; loop items run only when no regular head is ready; loop order/cursor and last-run timestamps are persisted; visual/current-item state is metadata, not prompt text; poll periods and event-frequency decisions stay deterministic until a later local-model dispatch hook is added.

Captured this as four backlog items: `#qloop1` syntax/state, `#qloop2` idle-loop sequencing, `#qloop3` poll controls, and `#qloop4` event-aware scheduling. No source files changed in this cycle.

Verification:
- `agent-doc preflight /home/brian/work/btakita/agent-loop/tasks/agent-doc/agent-doc-bugs2.md`
- `agent-doc read ... --component queue`
- `agent-doc read ... --component backlog`

Confidence: medium; this is a decomposition/architecture closeout for a multi-part feature, not an implementation patch.
Escalation: none.

### Re: telemetry observer for agent-doc bugs — gpt-5

> **Queue prompt:** How can we give agent-doc more telemetry access to identify bugs? Should we have an observer in the supervisor that takes periodic snapshots to process either with the subscription or a local model to identify bugs? Could we have an agent doc run a dedicated loop to schedule observation + bug fixes? So there would be a sidecar agent doc to this session that can observe all running project sessions to add bugs as they are seen. Could we also have a deterministic observer detect any annotated issues & deterministic patterns of issues?

Yes. The right shape is a two-layer observer: deterministic telemetry first, model triage second. The supervisor should publish compact snapshot artifacts for each live session; a deterministic observer should scan those snapshots plus `session-check`, preflight warnings, and ops-log markers for known issue fingerprints; a sidecar agent-doc loop can then file deduped bug backlog items without touching active panes. Model processing should consume compact snapshot/context-pack handles and remain advisory unless a deterministic rule or explicit operator gate proves the bug.

Captured this as four backlog items: `#qobs1` snapshot surface, `#qobs2` deterministic issue rules, `#qobs3` sidecar observer loop, and `#qobs4` model-assisted advisory triage. No source files changed in this cycle.

Verification:
- `agent-doc preflight /home/brian/work/btakita/agent-loop/tasks/agent-doc/agent-doc-bugs2.md`
- `agent-doc read ... --component queue`
- `agent-doc read ... --component backlog`

Confidence: medium; this is a design/backlog closeout for a cross-cutting observability feature, not implementation.
Escalation: none.

### Re: local-model queue item dispatch — gpt-5

> **Queue prompt:** Allow dispatching of queue items, such as loop items, use local models.

Agreed. The existing queue dispatch surface already supports `/model <tier>` as an inline command, but that only changes the model override for later harness work. The missing feature is per-item backend selection: a queue or loop item should be able to declare that it is eligible for a local-model executor, run through that backend, and still close through the same response/commit/session-check proof boundary as normal queue work.

Captured this as three backlog items: `#qlm1` backend-selection syntax, `#qlm2` local-model dispatcher, and `#qlm3` safety/telemetry gates. No source files changed in this cycle.

Verification:
- `agent-doc preflight /home/brian/work/btakita/agent-loop/tasks/agent-doc/agent-doc-bugs2.md`
- `sed -n '1,260p' src/agent-doc/src/queue_dispatch.rs`
- `rg -n "local model|/model|queue.*model" src/agent-doc ...`

Confidence: medium; existing `/model` support is confirmed, but local-model execution requires a new backend path.
Escalation: none.

### Re: post-closeout queue sync drift — gpt-5

Committed editor-applied queue-sync/boundary drift that arrived after the prior closeout. No source work was performed and no queue head is being answered by this repair cycle.

### Re: JB Run Agent Doc submit/start miss tracking — gpt-5

> **Queue prompt:** JB `Run Agent Doc` still not submitting. Can we track when this happens and automatically file a bug so I don't have to endlessly repeat myself when you fail to fix the issue? [dogfooding] Can we also have the supervisor detect if the turn started? And if the turn has not started, send Enter and continue retrying until the turn starts?

Yes. This belongs in the existing `#3ygp` bug, not as a second duplicate backlog item. I expanded `#3ygp` so it now covers both halves: automatic bug filing for repeated JetBrains route/dispatch misses, and supervisor-side turn-start proof with bounded Enter/submission retries before filing the evidence-backed bug.

No source files changed in this cycle.

Verification:
- `agent-doc preflight /home/brian/work/btakita/agent-loop/tasks/agent-doc/agent-doc-bugs2.md`
- `agent-doc read ... --component queue`
- `agent-doc read ... --component backlog`
- The parent repo has no workflow named `CI`.
- Latest `agent-doc` CI for this push was still `in_progress` when checked: run `28065394925`.
- `make install-full` installed the verified build from `/tmp/agent-doc-qstrikework`.
- Clean worktree `make check`: clippy, 5048 nextest tests, doctests, and medium simulator passed.
- `cargo test -p agent-doc --test test_cli flowcore_hot_path_guard_and_proof_tokens_are_budgeted -- --nocapture`
- `cargo test -p agent-doc finalize_keeps_free_text_queue_head_when_cycle_answers_foreign_exchange_prompt --test finalize_integration -- --nocapture`
- `cargo test -p agent-doc finalize_echoes_consumed_free_text_queue_prompt_into_response --test finalize_integration -- --nocapture`
- `cargo test -p agent-doc-orchestration consume_decision -- --nocapture`
- `cargo fmt --check`
- Parent repo `d0518761b` (`chore: update agent-doc submodule`), pushed to `origin/main`.
- `src/agent-doc` `b0a6f1eb` (`fix: require exact queue head proof before strike`), pushed to `origin/main`.
- Generic repair/no-op responses and unrelated exchange answers leave the active free-text queue head runnable.
- Id-backed heads accept explicit same-id closeout signals, including `--done`, `--pending-gate`, `--pending-edit`, and now `--review-resolve`.
- Free-text heads are consumed only when the current response explicitly quotes/targets that same queue prompt.
- `queue_consumption_allowed_for_response` now requires exact proof before striking a queue head.

Confidence: high for backlog capture; implementation remains queued as `do [#3ygp]`.
Escalation: none.

### Re: #3ygp

> **Queue prompt:** :pushpin: do [#3ygp]

Implemented `#3ygp`.

The JetBrains Run Agent Doc route/dispatch failure paths now file a deduped session-doc backlog bug when bounded submit or dispatch-start proof is exhausted. The auto-filed item includes the required evidence: failure class, document path, route stage, pane, actor generation, editor attempt id, dispatch proof state, elapsed time, preserved diagnostic path, ops log path, ops log marker, and a symptom-key marker keyed by document/stage/failure so repeated failures dedupe.

Coverage was wired into the direct-pane submit timeout path, supervisor IPC accepted-without-start-proof path, routed reopen accepted-without-start-proof path, and dispatch-only required proof failure path. Pane diagnostic snapshot paths are carried through the relevant submit/acceptance result structs. Docs were updated in `VERSIONS.md`, `editors/jetbrains/SPEC.md`, and `specs/08-session-routing.md`.

Verification:
- `cargo fmt --check`
- `cargo test -p agent-doc --test test_cli flowcore_hot_path_guard_and_proof_tokens_are_budgeted -- --nocapture`
- `cargo test -p agent-doc-orchestration route_dispatch_bug_report -- --nocapture`
- `cargo test -p agent-doc-orchestration direct_pane_enter_resubmit -- --nocapture`
- `cargo test -p agent-doc-orchestration route_submit -- --nocapture`
- `cargo test -p agent-doc-orchestration dispatch_only_direct_pane_accepted_submit_skips_optional_proof_wait -- --nocapture`
- `make check`
- `make install-full`

Committed and pushed source change `ef50ed8aa187839567552bcf627659099af383ba` (`fix: file route dispatch proof failures`) to `btakita/agent-doc` `main`. Updated and pushed the parent submodule pointer in `btakita/agent-loop` as `a51932f8d` (`chore: update agent-doc submodule`). GitHub Actions CI for the source push was queued as run `28066398449` when checked.

### Re: #fbwireverify

> **Queue prompt:** :pushpin: do [#fbwireverify]: "Operator-gated: needs two live editor/CLI panes the driving session cannot host." Please explain.

`#fbwireverify` is operator-gated because the remaining work is not code implementation. It is live acceptance proof for a race/transport behavior that only appears with a real editor plugin and a real agent pane running at the same time.

The test needs two independently controlled live surfaces: one editor/IDE side that can apply or withhold IPC acknowledgements, and one CLI/agent-doc pane that is being driven by the supervisor. The current driving session is itself the agent-doc turn being finalized, so using it as the test subject would collapse observer and subject into the same pane. It also cannot safely simulate a wedged editor consumer, slow editor ACKs, plugin reinstall/restart, and supervisor recycle while still preserving this session's own write/commit boundary.

What the operator has to verify later is concrete:

- Slow-ACK case: after install/recycle, drive a live editor + CLI pair and confirm `ops.log` shows `convergence_gate_defer` with `unmet=inflight_drained` before each dispatch, with no IPC inflight pileup above one.
- Wedged/no-ACK case: deliberately make the editor consumer stop acknowledging patches and confirm the bounded `CONVERGENCE_GATE_TIMEOUT_MS` path emits `convergence_gate_force_disk_fallback severity=error`, writes a loadable `.agent-doc/playback/<hash>/<cycle>.json` artifact, and the drain continues instead of sticking forever.

So the gate is about needing a human-controlled live topology and failure injection. Deterministic tests and `make check` already covered the shipped code path; this item stays open until that live proof is collected.

### Re: #fbwireverify gate cleanup

`#fbwireverify` remains open as live operator verification work. This cleanup keeps the explanation response committed while moving the underlying item into gated review, so the explanatory queue prompt is not re-served as runnable work and the live two-pane verification requirement is still preserved for the operator.
<!-- agent:boundary:64f4e774 -->
<!-- /agent:exchange -->

## Queue

<!-- agent:queue priority preset="#spec-test-build-install-commit-push" priority go -->
- ~~:pushpin: do [#3ygp]~~
- ~~:pushpin: do [#fbwireverify]: "Operator-gated: needs two live editor/CLI panes the driving session cannot host." Please explain.~~
- :pushpin: do [#qnodemerge4verify]: New plugin installed. How can I verify UTF-8 during merge? When does this happen? If this is too difficult, just mark this as done. 日本 😀日本 😀日本 😀日本 😀
- :pushpin: do [#tb4q]: please check logs
- :pushpin: do [#qcm1verify]: Use the logs to verify.
- ~~:pushpin: do [#qstrikework]~~
- :pushpin: do [#1jec]
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
- [ ] [#qnodemerge4verify] [operator-verify] #agent-doc-bug #qnodemerge4wire Phase 4 live non-ASCII verification (agent-doc submodule 4650054c; base-hash FFI + JB 0.2.173 + VS Code 0.2.29 reporters committed+pushed, make check + JB gradle + tsc/tests green, cdylib lib-installed). REMAINING operator-gated: (1) install the JB plugin zip from disk (build/distributions/agent-doc-jetbrains-0.2.173-signed.zip) + restart IDEA, and install the VS Code .vsix (editors/vscode/agent-doc-0.2.29.vsix) + reload window — the Kotlin/TS reporter changes need a plugin reinstall, the cdylib FFI is already hot-reloaded. (2) In a live editor, type a NON-ASCII edit (e.g. insert/delete around "café 日本 😀") into a session doc while an agent merge runs; confirm ops.log shows editor_op_recorded kind=insert/delete with BYTE offsets (not UTF-16) and that the concurrent merge aligns to the real edit boundary (no character interleaving / mojibake). The base-hash gate + replay==theirs gate make a misaligned op fall back to the diff-guess safely, so a wrong offset cannot corrupt — verification confirms the ops are ACCEPTED (editor_ops_for_base match), not merely safe.
- [ ] [#tb4q] [operator-verify] #qfocsupverify #agent-doc-bug Live-verify #qfocsup supervisor clear-and-continue for [focused-cycle] heads (agent-doc submodule 67829f3f; committed, make check green, NOT installed per STOP-cutting-releases). After make install + agent-doc admin recycle: with a [focused-cycle] head at the queue head and no in-session-drainable work, confirm the supervisor idle-watch force-clears the session (/clear) and re-dispatches the head to a fresh session instead of going idle. Proof: session-check ui_outcome=deferred_for_supervisor_drain on the yielding turn, then ops.log context_reset + go_drain_dispatch + a fresh agent draining the head.
- [ ] [#qcm1verify] [operator-verify] #agent-doc-bug Install + live-verify the #qnodemerge1 component-scoped merge (agent-doc submodule 52458f47). Code shipped+pushed, make check green (4853). Operator-gated remainder (mid-session install causes finalize-client-newer drift; STOP-cutting-releases gate): at a supervisor-recycle boundary run make install (cargo install --path . + cargo build --release + agent-doc lib-install), agent-doc admin recycle, then drive one edit-during-finalize cycle (type into a queue item while the agent writes an exchange response) and confirm ZERO cross-component splice + no console-output-in-queue corruption.
- [ ] [#1jec] [operator-verify] #yzerverify #agent-doc-bug Live-verify the #yzer/#evmhplugin plugin reconnect re-read (agent-doc 0.34.28 / submodule f6b8ae17, plugin 0.2.172). Code shipped+pushed: binary FFI agent_doc_reconnect_buffer_decision (disk==HEAD via ffi_show_head + buffer==prior-commit via ffi_show_prior_blobs) + pure decide_reconnect_buffer + PatchWatcher.registerRoot reconcile-on-connect (applyReconnectReread) + SimWorld coverage; make check green (4832). Operator-gated remainder (install is unsafe mid-session = finalize-client-newer drift): at a supervisor-recycle boundary run make install (cargo install --path . + cargo build --release + agent-doc lib-install), reinstall plugin 0.2.172, agent-doc admin recycle. Then repro: open a session doc in JB, kill/restart the supervisor so the binary commits while the plugin is disconnected, and on reconnect confirm ops.log shows 'reconnect_buffer_decision decision=reread_disk #yzer' and the editor buffer == HEAD with NO File Cache Conflict; a buffer holding genuine unsaved user edits must instead log decision=keep_buffer and be preserved (editor wins).
<!-- /agent:backlog -->

## Review
 
<!-- agent:review -->
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
