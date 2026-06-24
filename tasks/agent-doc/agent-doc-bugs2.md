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

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7a9b103bd5b68223dac05a4c449ac03899aba53d6022e82100578b5b0-20260624-030802.md`*

Compacted content:
- Archived 4 response topic(s): response overwrite sidecar recovery - gpt-5; #advr02 - gpt-5; state backbone - gpt-5; 1 more
- Prior summary/context: prior compacted content: Archived prior response topics and recovery notes from this long-running agent-doc session.;...

### Re: state backbone queue closeout - gpt-5

> **Queue prompt:**
>
> Fix Insufficient IPC Proof issues.


> **Queue prompt:**
>
> I want to have a backbone for all state. We should keep the Cycle State Machine. But for the rest of the state would any be a good match?

Closed this through the supported queue-response path after repairing the corrupted exchange component.

The answer and implementation are now both present in the session document and repo history: keep the Cycle State Machine for the global turn lifecycle, and use typed append-only events plus deterministic projections and actor ownership for the rest of agent-doc state. FSMs remain useful for small closed subdomains; behavior-tree-like logic is only a policy layer; GOAP and MPC are not a good durable-state backbone for this workflow.

Committed docs/spec work:

- `src/agent-doc`: `b3239b70 docs: define state backbone`
- Parent repo: `35659b5a0 chore: update agent-doc submodule`
- Session exchange repair: `7dbe13d66 agent-doc(agent-doc-bugs2): 2026-06-23 22:57:52`

Verification already completed:

- `make check` in `src/agent-doc` passed with Clippy, 5053 nextest tests, doctests, and the wider closeout sim.
- `gh run list --workflow CI --limit 1` reported latest `CI` as `completed success`, run `28070395022`, on `2026-06-24T02:11:52Z`.

No new backlog item is needed from this response.

### Re: state backbone implementation — gpt-5

> **Queue prompt:**
>
> Implement state backbone /home/brian/work/btakita/agent-loop/src/agent-doc/specs/13-state-backbone.md. Add additional state machines for other state domains.

Implemented the state backbone as a concrete orchestration module instead of leaving the spec as prose.

Changes:

- Added `agent-doc-orchestration/src/state_backbone.rs` with a typed append-only `EventLedger`, `StateEvent`, `StateFact`, and deterministic `StateBackboneProjection` reducers for document, queue, closeout, transport, supervisor, route, and proof state.
- Kept the existing `CyclePhaseMachine` as the closeout lifecycle authority and added local FSMs for other closed domains: `QueueHeadMachine`, `TransportPatchMachine`, `ActorLifecycleMachine`, `RouteReadinessMachine`, and `ProofGateMachine`.
- Added generation-aware owner checks so stale editor IPC / route / supervisor reports are recorded as rejected instead of mutating current projections.
- Updated `specs/13-state-backbone.md` to point at the implementation and list the state machines now backing the spec.

Verification completed before closeout:

- `cargo fmt --check`
- `cargo test -p agent-doc-orchestration state_backbone -- --test-threads=2`
- `cargo clippy -p agent-doc-orchestration -- -D warnings`
- `make check` (clippy, 5056 nextest tests, doctests, medium SimWorld)
- `make install-full` (installed `agent-doc 0.34.43`; lib install marked one controller for recycle at next idle boundary)
- `gh run list --workflow CI --limit 1` showed latest `CI` run `28070395022` completed successfully.

Local commits prepared:

- `src/agent-doc`: `a565bb49 feat: add state backbone projections`
- parent repo: `4105af85a chore: update agent-doc submodule`

Per the session-doc closeout ordering, I will push after this `agent-doc finalize` commit lands.

### Re: lazily-rs state machines — gpt-5

> **Queue prompt:**
>
> Implement state backbone /home/brian/work/btakita/agent-loop/src/agent-doc/specs/13-state-backbone.md. Add additional state machines for other state domains.

> **Queue prompt:**
>
> Are you using lazily-rs for all state machines?

The state-backbone implementation is complete and pushed.

Yes for the closed FSM domains introduced in this change: the existing `CyclePhaseMachine` and the new `QueueHeadMachine`, `TransportPatchMachine`, `ActorLifecycleMachine`, `RouteReadinessMachine`, and `ProofGateMachine` are all backed by `lazily::ThreadSafeStateMachine`.

Not every state surface is a lazily FSM. The backbone itself is event-driven: `EventLedger` records typed facts, and `StateBackboneProjection` reduces those facts into current document, queue, closeout, transport, supervisor, route, and proof projections. Those reducers are deterministic replay logic, not finite state machines. Existing broader projections such as SimWorld and some runtime enums can remain plain reducers/enums until a closed subdomain needs an explicit transition table.

Push / CI state after the implementation:

- `src/agent-doc` pushed `a565bb49 feat: add state backbone projections` to `origin/main`.
- Parent repo pushed through merge commit `903f4904a` to `origin/main`, including `4105af85a chore: update agent-doc submodule` and the session closeout commit.
- Latest `agent-doc` CI after the push is running: `28072965944`, started `2026-06-24T03:25:07Z`.

### Re: linking state machines — gpt-5

> **Queue prompt:**
>
> Can we link the different State Machines together? Is that advisable or should we keep them discrete?

Keep the state machines discrete as transition authorities, but link them through events and projections.

That means one FSM should not directly drive another FSM's internal transition table. Instead, it emits a typed event such as `EditorAckObserved`, `ActorGenerationChanged`, or `DispatchProofObserved`; projections reduce those events; policy code reads the projections and decides whether to emit the next command/event. This keeps each FSM small and replayable while still allowing the whole workflow to coordinate.

Good links:

- Shared event ledger with stable ids and causation ids.
- Projection dependencies, for example route dispatch reads supervisor readiness and transport proof projections.
- Policy layer / behavior-tree-like decision code that reads projections and emits events or commands.
- Explicit owner generation checks at projection boundaries.
- Parent repo: `33a085c3a chore: update agent-doc submodule` (created locally; parent push happens after this session closeout commit lands).
- `src/agent-doc`: `742ec733 feat: log restart agent menu marker` (pushed to `origin/main`).
- `gh run list --workflow CI --limit 1` reported latest `CI` as `completed success`, run `28072965944`, from `2026-06-24T03:25:07Z`.
- Installed jar metadata was verified: `META-INF/plugin.xml` reports version `0.2.180` and registers `AgentDoc.RestartAgent`.
- `agent-doc plugin install --local jetbrains` installed signed `agent-doc-jetbrains 0.2.180` to `/home/brian/.local/share/JetBrains/IntelliJIdea2026.1`; restart IntelliJ to activate it.
- `make install-full` installed `agent-doc 0.34.43` and `libagent_doc-0.34.43.so`; lib install marked two controllers to recycle at the next idle boundary.
- `make check` twice, including the post-doc-edit rerun: clippy, 5056 nextest tests, doctests, and medium SimWorld all passed.
- `GRADLE_USER_HOME=/tmp/agent-doc-gradle ./gradlew test buildPlugin signPlugin`
- `GRADLE_USER_HOME=/tmp/agent-doc-gradle ./gradlew test --tests com.github.btakita.agentdoc.TerminalUtilTest`
- `cargo test -p agent-doc-core gate_verify -- --test-threads=2`
- `cargo test -p agent-doc-orchestration gate_verify -- --test-threads=2`
- `git diff --check`
- `cargo fmt --check`

Avoid links where one FSM reaches into another FSM's mutable state or encodes another domain's transitions. That recreates a global FSM by accident and brings back state explosion. The right shape is discrete FSMs plus typed events, deterministic projections, and policy orchestration above them.

### Re: #advr03 — gpt-5

> **Queue prompt:**
>
> do [#advr03]

Completed `#advr03`.

Implemented the missing `#restartagentmenu` proof marker so the JetBrains Restart Agent menu path now leaves structured evidence for `#optverify` instead of only an IDE logger line. `RestartAgentAction` records the marker before invoking the restart command; `TerminalUtil` writes a best-effort `restart_agent_menu_invoked file=<relative-path> source=jetbrains action=restart_agent` line to `.agent-doc/logs/ops.log`; tests pin both the action ordering and marker format. `specs/supervisor.md` and `VERSIONS.md` now document the marker contract, and the JetBrains plugin version is bumped to `0.2.180`.

Verification:

- `cargo fmt --check`
- `git diff --check`
- `cargo test -p agent-doc-orchestration gate_verify -- --test-threads=2`
- `cargo test -p agent-doc-core gate_verify -- --test-threads=2`
- `GRADLE_USER_HOME=/tmp/agent-doc-gradle ./gradlew test --tests com.github.btakita.agentdoc.TerminalUtilTest`
- `GRADLE_USER_HOME=/tmp/agent-doc-gradle ./gradlew test buildPlugin signPlugin`
- `make check` twice, including the post-doc-edit rerun: clippy, 5056 nextest tests, doctests, and medium SimWorld all passed.
- `make install-full` installed `agent-doc 0.34.43` and `libagent_doc-0.34.43.so`; lib install marked two controllers to recycle at the next idle boundary.
- `agent-doc plugin install --local jetbrains` installed signed `agent-doc-jetbrains 0.2.180` to `/home/brian/.local/share/JetBrains/IntelliJIdea2026.1`; restart IntelliJ to activate it.
- Installed jar metadata was verified: `META-INF/plugin.xml` reports version `0.2.180` and registers `AgentDoc.RestartAgent`.
- `gh run list --workflow CI --limit 1` reported latest `CI` as `completed success`, run `28072965944`, from `2026-06-24T03:25:07Z`.

Commits:

- `src/agent-doc`: `742ec733 feat: log restart agent menu marker` (pushed to `origin/main`).
- Parent repo: `33a085c3a chore: update agent-doc submodule` (created locally; parent push happens after this session closeout commit lands).

Escalation: none. Confidence: high.

### Re: #advr03 queue closeout repair — gpt-5

The `#advr03` implementation response and backlog reap are already committed. This repair closeout persists the queue strike that preflight converged after the IPC retry, so the visible queue now advances to `#advr04` without reopening any code work.

No additional repo code changed after the verification recorded in the `#advr03` response. Backlog now starts at `#advr04`; the done archive has a local `#advr03` append alongside older uncommitted archive drift, so I did not stage that archive wholesale.

### Re: #advr04 — gpt-5

> **Queue prompt:**
>
> do [#advr04]

Completed `#advr04` as already implemented by the immediately preceding restart-agent work; I did not add duplicate source changes in this cycle.

Confirmed surfaces:

- JetBrains `RestartAgentAction.kt` exists and records `restart_agent_menu_invoked` before calling `TerminalUtil.restartSession`.
- `plugin.xml` registers `AgentDoc.RestartAgent` with text `Restart Agent`, alongside `Restart Supervisor Process` and `Stop Agent`.
- `TerminalUtil` writes the optverify-scannable ops-log marker `restart_agent_menu_invoked file=<relative-path> source=jetbrains action=restart_agent`.
- `editors/jetbrains/gradle.properties` is at `pluginVersion = 0.2.180`.
- Signed and unsigned plugin artifacts exist: `agent-doc-jetbrains-0.2.180.zip` and `agent-doc-jetbrains-0.2.180-signed.zip`.
- Supervisor-side harness-change proof is present: `harness_change_detected`, `agent_restart_boundary_gate`, `agent_restart_performed`, plus the route kill-guard test for avoiding cold replacement of a healthy live authoritative actor.

Verification run for this closeout:

- `cargo test -p agent-doc-orchestration agent_change_restart -- --test-threads=2` passed.
- `cargo test -p agent-doc-orchestration mismatched_authoritative_actor_can_be_replaced_only_when_not_live_authority -- --test-threads=2` passed.
- `cargo test -p agent-doc-orchestration session_log_event_after_latest_start_resets_on_agent_restart_performed -- --test-threads=2` passed.
- `GRADLE_USER_HOME=/tmp/agent-doc-gradle ./gradlew test buildPlugin signPlugin` passed.
- `git diff --check` passed.
- `gh run list --workflow CI --limit 1` reported latest `CI` as `completed success`, run `28073513496`, for `feat: log restart agent menu marker`.

No new source commit was needed. `src/agent-doc` had unrelated dirty JetBrains style-setting files appear during verification (`VisualHighlighterManager.kt`, `MarkdownStyleConfigurable.kt`, `MarkdownStyleSettings.kt`); I left them untouched and did not stage them for this closeout.

Escalation: none. Confidence: high.

### Re: plugin version note — gpt-5

> **Queue prompt:**
>
> Another session bumped JB plugin to 0.1.181. So you may need to bump to 0.1.182.

Confirmed the current JetBrains plugin version in `src/agent-doc/editors/jetbrains/gradle.properties` is `0.2.181`, from clean submodule commit `28ba4850 feat(jetbrains): per-structure Markdown style overrides (#lzmdstyle, #9xtw)`.

So the actionable version guidance for the next JetBrains plugin-bearing change is: bump `pluginVersion` from `0.2.181` to `0.2.182`. I treated the `0.1.x` version in the queue note as stale/typoed shorthand and made no source edits in this cycle.

Escalation: none. Verification: inspected `gradle.properties`, `src/agent-doc` status, and the submodule commit. Confidence: high.

### Re: #advr05 — gpt-5

> **Queue prompt:**
>
> do [#advr05]

Completed `#advr05` as already implemented in the current `src/agent-doc` tree; I made no duplicate source edits in this cycle.

Confirmed surfaces:

- JetBrains has `StopAgentAction.kt`, and `plugin.xml` registers `AgentDoc.StopAgent` with text `Stop Agent` alongside `Restart Agent` and `Restart Supervisor Process`.
- `TerminalUtil.stopAgent` routes to `agent-doc session stop-agent` and keeps the supervisor running so `Restart Agent` can bring the harness back.
- VS Code contributes `agentDoc.restartAgent` and `agentDoc.stopAgent`, registers both commands, and routes them through the same session command helpers.
- Current JetBrains plugin version is `0.2.181`, and signed/unsigned artifacts exist: `agent-doc-jetbrains-0.2.181.zip` and `agent-doc-jetbrains-0.2.181-signed.zip`.
- `src/agent-doc` is clean at `28ba4850 feat(jetbrains): per-structure Markdown style overrides (#lzmdstyle, #9xtw)`.

Verification run for this closeout:

- `GRADLE_USER_HOME=/tmp/agent-doc-gradle ./gradlew test buildPlugin signPlugin` passed.
- `npm test` in `src/agent-doc/editors/vscode` passed: 98 tests.
- `git diff --check` in `src/agent-doc` passed.
- `gh run list --workflow CI --limit 1` reported latest `CI` as `completed success`, run `28073970198`, for `feat(jetbrains): per-structure Markdown style overrides (#lzmdstyle, …)`.

Escalation: none. Confidence: high.
❯ **IPC proof issue dogfood log**

❯ Detected during interrupted-cycle recovery. This note is appended automatically so the session document records editor IPC issues while queue edits are being dogfooded.

❯ Issue class: `ipc_proof_insufficient`
❯ Affected component: editor IPC / writeback

```text
ipc_proof_insufficient file=tasks/agent-doc/agent-doc-bugs2.md source=socket_ack_content patch_id=32e760c5-a607-4a96-b776-98cc062b6cc4 invariant=live_prompt_drift_after_preflight recovery=content_ours_snapshot_next_cycle snap_source=ack_content_sidecar candidate_len=25214 candidate_hash=c3433aa04952120e58f4136d3a90a302e09953aed26db13539dfdd1b55080e9b content_ours_len=25505 content_ours_hash=c02dfe614ab52ce2cd3384e6f69602619d8e3c0db155067111939cc7d43d105c doc=agent-doc-bugs2 session=b26b9957-8de6-4eae-bb98-5d7fe4d6b781 turn=cycle-1782273729807
```

### Re: #advr06 — gpt-5

Implemented the `#recyclerename` follow-up.

Changed surfaces:
- JetBrains `AgentDoc.RestartSupervisorProcess` keeps its stable action ID but now shows `Recycle Supervisor`; `Restart Agent` is adjacent in the JetBrains popup/action ordering, and `Stop Agent` remains adjacent in the IDE menu groups.
- VS Code `agentDoc.restartSession` keeps its stable command ID but now shows `Agent Doc: Recycle Supervisor`; the command palette, editor context menu, and Alt+Enter QuickPick now place `Recycle Supervisor` beside `Restart Agent` where that lifecycle action is present.
- The underlying CLI remains `agent-doc session restart-supervisor`; fallback success hints now say the recycle was requested.
- JetBrains `pluginVersion` is bumped to `0.2.182`; signed and unsigned `0.2.182` zips were built.
- Editor specs, VS Code README, and `VERSIONS.md` are aligned with the rename/stable-ID contract.

Verification:
- `npm test` in `src/agent-doc/editors/vscode`: 98 tests passed.
- `GRADLE_USER_HOME=/tmp/agent-doc-gradle ./gradlew test buildPlugin signPlugin` in `src/agent-doc/editors/jetbrains`: passed.
- `make check` in `src/agent-doc`: passed, including clippy, 5056 nextest tests, doctests, and the medium closeout sim.
- `git diff --check` / cached diff checks passed.
- `gh run list --workflow CI --limit 1` reported latest `CI` as `completed success`, run `28073970198`.
- Local JetBrains plugin install used the built `agent-doc-jetbrains-0.2.182-signed.zip` and installed to `/home/brian/.local/share/JetBrains/IntelliJIdea2026.1`; IntelliJ needs a restart to load it.

Commits/push:
- `src/agent-doc`: `583e1363 feat(editors): rename supervisor restart action`, pushed to `origin/main`.
- parent repo: `86f9eb356 chore: advance agent-doc submodule`.

### Re: IPC proof issue dogfood log — gpt-5

> **Queue prompt:**
>
> :pushpin: do [#advr06]


Acknowledged. The preflight recovery appended an `ipc_proof_insufficient` dogfood note for the prior interrupted-cycle recovery; this turn continued through the binary-owned finalize path instead of direct-editing the session document.

Escalation: none. Confidence: high.

### Re: #advr07 — gpt-5

> **Queue prompt:**
>
> :pushpin: do [#advr07]


Verified `#supinstallvis` / `#supinstallfeedback` is already implemented in the current `src/agent-doc` submodule and closes this queue item.

Evidence:
- Implementation commit: `cf7347a93 feat(#supinstallfeedback): surface supervisor auto-install status on the owned pane`.
- The idle supervisor uses a non-input-consuming `tmux display-message` targeted at the owned pane for auto-install phases, so it does not type into the harness composer or disturb the `Press Enter to restart` keepalive reader.
- The started message explicitly says the supervisor is rebuilding and says `do NOT press Enter`; success and failure messages are distinct terminal phases.
- The auto-install path records session-log and ops-log proof markers: `supervisor_auto_install_started`, `supervisor_auto_install_succeeded`, and `supervisor_auto_install_failed`.
- The opt-out detect path still logs `supervisor_source_newer_detected` and prints manual refresh guidance instead of silently stalling.
- `VERSIONS.md` documents the dogfood supervisor auto-install behavior and the pane/status feedback contract.

Verification this cycle:
- `cargo test -p agent-doc-orchestration supervisor_auto_install`: passed 2 tests, including the pane-message keepalive warning and auto-install default-on precedence.
- `cargo test -p agent-doc-orchestration supervisor_install_action_policy`: passed.
- `git diff --check` in `src/agent-doc`: passed.
- `git status --short` in `src/agent-doc`: clean.
- `gh run list --workflow CI --limit 1`: latest CI for `feat(editors): rename supervisor restart action` was still `in_progress`, so I recorded it as pending rather than waiting.

No source changes or new repo commits were needed for this item.

Escalation: none. Confidence: high.

### Re: #advr08 — gpt-5

> **Queue prompt:**
>
> :pushpin: do [#advr08]


Verified `#runexitrestart` is already implemented in the current `src/agent-doc` submodule and closes this queue item.

Evidence:
- Implementation commit: `1e73e7cb route(#runexitrestart): gate restart-dispatch behind harness dispatch-ready prompt`.
- The supervisor idle-watch drain no longer trusts the weak edge-triggered pty prompt glyph after a restart. When the actor is not yet `Ready`, it re-verifies with a fresh tmux capture through `supervisor_pane_dispatch_ready` and the same `ready_prompt_candidate` dispatch-ready predicate used by route/cold-start gates.
- While the restarted harness is still coming up, the drain fails closed and records `dispatch_into_restarting_pane reason=harness_not_dispatch_ready_before_restart_drain_send` instead of typing into the composer.
- Once a dispatch-ready prompt is observed, the same drain submits exactly once. The SimWorld regression asserts no injected trigger across seven not-ready ticks, then a single `dispatch_inject attempt=1` after readiness proof.
- `VERSIONS.md` documents the restart-drain fix and its live-proof expectations.

Verification this cycle:
- `cargo test -p agent-doc route_sim_restart_drain_waits_for_dispatch_ready_prompt_before_send`: passed.
- `cargo test -p agent-doc route_sim_auto_start_dispatch_waits_for_dispatch_ready_prompt_before_send`: passed.
- `git diff --check` in `src/agent-doc`: passed.
- `git status --short` in `src/agent-doc`: clean.
- `gh run list --workflow CI --limit 1`: latest CI completed successfully, run `28074608395`.

No source changes or new repo commits were needed for this item.

Escalation: none. Confidence: high.

### Re: #advr09 — gpt-5

> **Queue prompt:**
>
> :pushpin: do [#advr09]


Verified `#jbrundispatch` is already implemented in current `src/agent-doc` and closes this queue item.

Evidence:
- Dispatch proof failures file deduped session-doc bug items via `file_route_dispatch_bug_report` / `route_dispatch_bug_report_item`.
- The filed item includes deterministic route evidence: `#jbrunautobug #agent-doc-bug`, failure class, document, stage, pane, actor generation, editor attempt, dispatch proof state, elapsed time, diagnostic path, ops log path/marker, and symptom key.
- `require_dispatch_only_dispatch_start_proof` distinguishes accepted-only from dispatch-start proof, records route submit blocked/proof failed, logs the issue, and files the bug item.
- Route submit observations classify successful dispatch-start proof vs accepted-without-proof.
- Ops report grouping covers route dispatch proven/unproven events for deterministic diagnostics.
- Related shipped commits: `ef50ed8a`, `0549d935`, `4a9284d0`, `aca970fe`.

Verification:
- `cargo test -p agent-doc-orchestration route_dispatch_bug_report`: passed 2 tests.
- `cargo test -p agent-doc-orchestration route_submit_observation_marks`: passed 3 tests.
- `cargo test -p agent-doc ops_summary_groups_tracked_events_by_file_and_session`: passed.
- `git -C src/agent-doc diff --check`: clean.
- Latest CI: `28074608395` completed successfully on 2026-06-24.

No source changes or new repo commits were needed.

Escalation: none. Confidence: high.
<!-- agent:boundary:8fce0bb5 -->
<!-- /agent:exchange -->

## Queue

<!-- agent:queue priority preset="#spec-test-build-install-commit-push" priority go -->
- ~~:pushpin: do [#advr06]~~
- ~~:pushpin: do [#advr07]~~
- ~~:pushpin: do [#advr08]~~
- ~~:pushpin: do [#advr09]~~
- :pushpin: do [#advr10]
- :pushpin: do [#advr11]
- :pushpin: do [#advr12]
- :pushpin: do [#advr13]
- :pushpin: do [#advr14]
- :pushpin: do [#advr15]
- :pushpin: do [#advr16]
- :pushpin: do [#advr17]
- :pushpin: do [#advr18]
- :pushpin: do [#advr19]
- :pushpin: JB `Run Agent Doc` still does not quickly submit the prompt.
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
- [ ] [#advr10] Advance original #editorbufwin: finish editor-buffer-wins P2 by auto-recovering and committing preserved queue additions when replay normalization neutralizes the queue component. Plan: tasks/agent-doc/plan-editor-buffer-wins-queue-merge.md.
- [ ] [#advr11] Advance original #docdriftgrace: implement component-scoped graceful drift resolution that auto-rebases unrelated changes and surfaces only structural conflicts on in-flight prompt/response. Plan: tasks/agent-doc/plan-graceful-doc-drift-resolution.md.
- [ ] [#advr12] Advance original #ftqpartialdup: confirm whether #hap7/#gf5n fixes same-node partial free-text queue duplication; if not, add same-node/free-text conflict guard and regression for truncated duplicate queue prompts.
- [ ] [#advr13] Advance original #8bfz: implement single live JB plugin consumer per document using a per-doc owner lease; non-owner windows defer patch apply + saveDocument, with plugin PatchWatcher + binary lease primitive and multi-window proof.
- [ ] [#advr14] Advance original #qpauseux: finish paused-queue plugin UX with info/action notifications, Resume or restart-supervisor/resume actions, no raw error toast, plugin artifacts, and render/ops proof.
- [ ] [#advr15] Advance original #jbdisprecycle: convert recycle-boundary Run Agent Doc verification into deterministic proof for exactly one `dispatch_start_proof` with submit and no stranded start_session failure; ops marker `route_dispatch_submit_recycle_settle`.
- [ ] [#advr16] Advance original #tb4q: complete #qfocsup focused-cycle supervisor clear-and-continue proof with install/recycle if needed, `session-check ui_outcome=deferred_for_supervisor_drain`, `context_reset`, `go_drain_dispatch`, and fresh-agent drain evidence.
- [ ] [#advr17] Advance original #fbwireverify: complete #fbwire/#fullboundary acceptance with slow-ACK no inflight pileup and wedged no-ACK bounded timeout + ERROR marker + playback artifact; add deterministic tmux-ci/sim coverage where possible.
- [ ] [#advr18] Advance original #qnodemerge4verify: after JB/VS Code plugin install, live-test non-ASCII edit-during-merge around `café 日本 😀`; prove accepted byte-offset editor ops and no mojibake/interleaving, plus automated log checker for `editor_op_recorded` + `editor_ops_for_base`.
- [ ] [#advr19] Advance original #cyh0: identify and instrument remaining File Cache Conflict trigger by capturing exact action, agent-doc command, and ops marker such as `write_authority action=... transport=disk`; distinguish ack-content, VCS-refresh/save, and editor-IPC convergence surfaces.
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
