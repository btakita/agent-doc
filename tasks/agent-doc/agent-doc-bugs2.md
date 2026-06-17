---
agent_doc_session: b26b9957-8de6-4eae-bb98-5d7fe4d6b781
agent: codex
claude_model: opus
agent_doc_format: template
agent_doc_write: crdt
claude_args: --dangerously-skip-permissions
codex_args: -s danger-full-access
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

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7a9b103bd5b68223dac05a4c449ac03899aba53d6022e82100578b5b0-20260617-071418.md`*

Compacted content:
- Archived 1 response topic(s): There are still disk writes causing JB `File Cache Conflict` dialogs.
- Prior summary/context: ### Response — gpt-5 Fixed and installed the `agent-doc admin recycle` CLI path behind the screenshot. The clean-term...
- `cargo test -p agent-doc-orchestration direct_pane_existing_draft -- --nocapture`

### Re: Existing drafted Run Agent Doc input — gpt-5

> **Queue prompt:**
>
> :pushpin: If I run JB `Run Agent Doc` with the prompt already in the input, only send Return.

This head is already covered by the current installed route code; I did not make another source change for it.

Before appending a new routed trigger, `send_command_unchecked` captures the target pane and calls `direct_pane_existing_draft_visible`. When the same `agent-doc <FILE>` trigger is already visibly drafted in the active Codex/Claude/OpenCode composer, it takes the append-free path through `send_direct_pane_enter_resubmit`, which submits only the harness submit key (`Enter`) with an empty payload. A later idle prompt below the trigger classifies the text as stale scrollback, so route does not press Enter on old output.

Verification for this head passed:
- `cargo test -p agent-doc-orchestration direct_pane_existing_draft -- --nocapture`

No JetBrains plugin update is needed for this head. The behavior is in the installed `agent-doc 0.34.1` CLI/orchestration code.

### Re: Queue drain state — gpt-5

> **Queue prompt:**
>
> :pushpin: JB `Run Session Doc` did not submit. I then tried running JB `Clear Session Context`. It should be available to run. The supervisor should detect if the turn is running. If not, then send the `/clear`. If there is already text in the input, clear the input text and send `/clear` + Return.
> ```
> Clear Session Context is blocked while Run Agent Doc is dispatching for tasks/professional/equityfundingsource.md. Wait for dispatch to finish, or use Interrupt and Clear Session Context.
> ```


The recovered response for the existing drafted `Run Agent Doc` input is now committed. I did not make any source changes in this recovery closeout cycle.

The active queue currently has no in-session drainable head according to preflight (`queue_continuation_required: false`). The remaining `Run Session Doc` / `Clear Session Context` report needs live editor or operator-side verification before it should be drained from this session.

Changed paths: session document only, through the agent-doc repair/finalize path.
Commands: `agent-doc preflight /home/brian/work/btakita/agent-loop/tasks/agent-doc/agent-doc-bugs2.md`, `agent-doc plan ...`, `agent-doc session-check ...`.
Verification: the prior response was recovered and committed by preflight; this finalize is the closeout for the recovery cycle.
Escalation: none.


### Re: Queue drain state — gpt-5

The recovered response for the existing drafted `Run Agent Doc` input is now committed. I did not make any source changes in this recovery closeout cycle.

The active queue currently has no in-session drainable head according to preflight (`queue_continuation_required: false`). The remaining `Run Session Doc` / `Clear Session Context` report needs live editor or operator-side verification before it should be drained from this session.

Changed paths: session document only, through the agent-doc repair/finalize path.
Commands: `agent-doc preflight /home/brian/work/btakita/agent-loop/tasks/agent-doc/agent-doc-bugs2.md`, `agent-doc plan ...`, `agent-doc session-check ...`.
Verification: the prior response was recovered and committed by preflight; this finalize is the closeout for the recovery cycle.
Escalation: none.

### Re: JB Run Session Doc Codex submit proof — gpt-5

> **Queue prompt:**
>
> JB `Run Session Doc` did not submit to Codex on monsterrodholders.md. It was once working but now it is not. Do we have logging to isolate exactly why? Try polling Return until the submission occurs. I just want to see something work with Codex then we can refine the solution.


I implemented and pushed the Codex submit-proof fix in `src/agent-doc`.

What changed:
- `route.direct_pane_submit` now polls direct-pane acceptance and sends bounded `Enter` retries while the exact routed `agent-doc <FILE>` trigger remains visible in the composer. It stops when the trigger disappears or acceptance is observed, and logs `route_submit_resubmit ... attempt=N`.
- Existing drafted-trigger submits now use the same bounded retry loop, so a prefilled Codex/Claude/OpenCode input receives only the submit key, not duplicated text.
- I also fixed the adjacent CI failure in `session_clear`: supervisor IPC `/clear` verifies/retries only when the live supervisor reports its actor pane; if no pane id is reported, it logs `session_clear_submit_verification_skipped ... reason=no_supervisor_actor_pane` instead of failing on stale/default-tmux capture.

Live Codex evidence:
- The Codex-backed doc is `/home/brian/work/btakita/agent-loop/src/boost-client/tasks/monsterrodholders.md` (`agent: codex`, pane `%47`). The research doc with the same basename is Claude-backed, so I did not count it.
- The Monster Rod queue head was delivered to Codex on `%47`; ops logs show `route_dispatch_only_submit_unproven ... harness=codex delivery=direct_pane_submit proof=accepted`, then Codex committed `66e6ba23` in `src/boost-client` and root pointer `712b8d89d`. `agent-doc session-check` for that doc is clean (`no_drainable_work`).
- After final install I retried dispatch, but it did not inject because route still saw the supervisor actor as `starting` (`route_authoritative_actor_starting_not_ready ... runtime_state=ready prompt_ready=false`). I did not force a new queue mutation just for a second proof; the final binary is covered by local tests.

Verification:
- `cargo fmt`
- `cargo test -p agent-doc-orchestration direct_pane -- --nocapture`
- `cargo test -p agent-doc-orchestration route_submit_resubmit -- --nocapture`
- `cargo test --all-targets session_actor_cmd::tests::clear_falls_back_to_supervisor_inject_when_authoritative_pane_is_not_on_default_tmux -- --ignored --test-threads=1 --nocapture`
- `make check`
- `make tmux-ci`
- `make install` installed CLI + `libagent_doc.so` and marked controllers for idle-boundary recycle.

Commits:
- `src/agent-doc`: `0c95fd9b fix codex submit proof retries` pushed to `origin/main`.
- root pointer: `7045a5e7c agent-doc: update submit proof fixes` committed locally; root push remains after session closeout.

CI: latest `src/agent-doc` CI for `0c95fd9b` was `in_progress` when checked. The previous red `make tmux-ci` failure is fixed locally.

New queue note: I saw the File Cache Conflict queue item and ran `make install` to remove the stale cdylib/install warning. I did not implement the broader external-write ban in this closeout; that remains the next queue head.

Changed paths: `src/agent-doc/README.md`, `src/agent-doc/VERSIONS.md`, `src/agent-doc/agent-doc-orchestration/src/route/dispatch.rs`, `src/agent-doc/specs/08a-session-actor-contract.md`, `src/agent-doc/specs/11-debounce.md`, `src/agent-doc/src/session_actor_cmd.rs`, plus root `src/agent-doc` submodule pointer.
Escalation: none.

### Re: JB File Cache Conflict / external file edits — gpt-5

> **Queue prompt:**
>
> :pushpin: Still getting JB `File Cache Conflict` dialogs. Sometimes they happen frequently. Ban external file edits when the plugin is running.


Implemented and installed the binary-side ban on direct document writes while an editor plugin listener is running.

What changed:
- Shared editor convergence now allows disk fallback only when no JetBrains/VS Code IPC listener is active. Active-listener failures log `<source>_writeback ... transport=blocked reason=... action=refuse_external_disk_write` and fail closed.
- The guarded document gate, plain maintenance gate, queue consume, template apply, compact, pending/review/dedupe/preflight maintenance, and live-prompt auto-recovery paths inherit the same rule.
- Already-current active-listener updates are no-op successes (`transport=already_current`).
- Specs, runbooks, changelog, and workflow-invariant vocabulary now describe the no-listener-only disk fallback contract.

Verification:
- `cargo fmt --check`
- `cargo test -p agent-doc-orchestration write::converge::core_tests -- --nocapture`
- `cargo test --test test_cli flowcore_hot_path_guard_and_proof_tokens_are_budgeted -- --nocapture`
- `make check` (4635 passed, 238 skipped, doctests ok, ignored medium sim ok)

Install / commits:
- `src/agent-doc`: `59d2d9e5 block editor-active disk fallbacks` pushed to `origin/main`.
- root pointer: `f5ed3a04c agent-doc: block editor-active disk fallbacks` committed locally.
- `make install` installed the CLI and `libagent_doc.so`, marking 2 controllers for idle-boundary recycle.
- Latest CI for `59d2d9e5` was `in_progress` when checked.

No JetBrains plugin update is needed; this is Rust binary/orchestration behavior. The live route-owned supervisor for this doc still warns stale until the current busy turn reaches an idle boundary and recycles.

Changed paths: `src/agent-doc/agent-doc-orchestration/src/write/converge.rs`, `src/agent-doc/agent-doc-orchestration/src/test_support.rs`, `src/agent-doc/agent-doc-orchestration/src/write/run_entry.rs`, `src/agent-doc/agent-doc-orchestration/src/compact.rs`, `src/agent-doc/agent-doc-orchestration/src/flow/workflow_invariants.rs`, `src/agent-doc/tests/test_cli.rs`, `src/agent-doc/VERSIONS.md`, `src/agent-doc/specs/07-closeout-commands.md`, `src/agent-doc/specs/07-orchestration-commands.md`, `src/agent-doc/runbooks/commit.md`, `src/agent-doc/runbooks/compact-exchange.md`, `src/agent-doc/runbooks/jb-cache-conflict.md`, plus root `src/agent-doc` submodule pointer.
Escalation: none.
Implemented, installed, committed, and pushed.

The Rust orchestration now refuses direct disk fallback while an editor/plugin IPC listener is active. Active-listener failures now fail closed with `transport=blocked ... action=refuse_external_disk_write`; no-listener fallback still works. Live-prompt auto-recovery also blocks its adopted-snapshot disk write unless editor convergence is proven.

Verification passed:
- `cargo fmt --check`
- focused convergence/recovery tests
- `make check` with 4635 passed, 238 skipped
- `src/agent-doc` GitHub Actions `CI` completed successfully for `59d2d9e5`

Commits pushed:
- `src/agent-doc`: `59d2d9e5 block editor-active disk fallbacks`
- root pointer: `f5ed3a04c agent-doc: block editor-active disk fallbacks`
- session closeout: `01e3cbbe4`, `d4502c96c`
- JetBrains plugin `0.2.169` is installed. Restart IntelliJ IDEA 2026.1 to activate the plugin bytecode.
- CLI/cdylib installed with `make install`; 2 controllers were marked for idle-boundary recycle.
- root: `3839e1512 agent-doc: update jetbrains clear preemption` pushed to `origin/main`.
- `src/agent-doc`: `4ea5f4b9 fix jetbrains clear preempts stalled dispatch` pushed to `origin/main`.
- root `agent-loop` has no workflow named `CI`.
- `src/agent-doc` latest CI for `4ea5f4b9` was `in_progress` when checked.
- `agent-doc plugin install --local jetbrains` for `/home/brian/.config/JetBrains/IntelliJIdea2026.1` and `/home/brian/.local/share/JetBrains/IntelliJIdea2026.1`
- `make install`
- `./gradlew test`
- `make check` (`clippy`, 4635 Rust tests, doctests, medium simulator, version sync)
- `make bump-plugin`
- `./gradlew test --tests com.github.btakita.agentdoc.EditorCommandStateMachineTest --tests com.github.btakita.agentdoc.TerminalUtilTest`
- `cargo test -p agent-doc-orchestration route_submit_resubmit_proof_line_is_operator_greppable_for_both_harnesses -- --nocapture`
- `cargo fmt --check`
- root `src/agent-doc` gitlink
- `src/agent-doc/VERSIONS.md`
- `src/agent-doc/editors/jetbrains/gradle.properties`
- `src/agent-doc/editors/jetbrains/SPEC.md`
- `src/agent-doc/agent-doc-orchestration/src/route/dispatch.rs`
- `src/agent-doc/editors/jetbrains/src/test/kotlin/com/github/btakita/agentdoc/TerminalUtilTest.kt`
- `src/agent-doc/editors/jetbrains/src/test/kotlin/com/github/btakita/agentdoc/EditorCommandStateMachineTest.kt`
- `src/agent-doc/editors/jetbrains/src/main/kotlin/com/github/btakita/agentdoc/TerminalUtil.kt`
- `src/agent-doc/editors/jetbrains/src/main/kotlin/com/github/btakita/agentdoc/EditorCommandStateMachine.kt`
- JetBrains plugin version bumped to `0.2.169`, signed/unsigned plugin zips rebuilt, and `0.2.169` installed into both IntelliJ IDEA 2026.1 plugin locations found on this machine.
- Route submit proof coverage now explicitly includes OpenCode alongside Codex and Claude.
- Repeated Run clicks still dedupe behind the first alive route process. Run clicked during an active Clear still queues until synchronous Clear completion.
- JetBrains `Clear Session Context` now preempts a still-dispatching `Run Agent Doc` route instead of blocking. The plugin cancels the active route handle and then runs the normal binary-owned `agent-doc session clear <FILE>` path, so prompt correction, Enter submit retries, and protected-input refusal stay in shared Rust behavior.

`make install` installed the new CLI/cdylib and marked 2 controllers for idle-boundary recycle. No JetBrains plugin code update was needed. The only remaining warning is that the current route-owned supervisor is still mapping the stale binary until it reaches an idle recycle boundary; I did not force-restart the busy pane.

### Re: JB Clear while Run dispatch is stalled — gpt-5

Implemented, installed, committed, and pushed.

What changed:
- JetBrains `Clear Session Context` now preempts a still-dispatching `Run Agent Doc` route instead of blocking. The plugin cancels the active route handle and then runs the normal binary-owned `agent-doc session clear <FILE>` path, so prompt correction, Enter submit retries, and protected-input refusal stay in shared Rust behavior.
- Repeated Run clicks still dedupe behind the first alive route process. Run clicked during an active Clear still queues until synchronous Clear completion.
- Route submit proof coverage now explicitly includes OpenCode alongside Codex and Claude.
- JetBrains plugin version bumped to `0.2.169`, signed/unsigned plugin zips rebuilt, and `0.2.169` installed into both IntelliJ IDEA 2026.1 plugin locations found on this machine.

Changed paths:
- `src/agent-doc/editors/jetbrains/src/main/kotlin/com/github/btakita/agentdoc/EditorCommandStateMachine.kt`
- `src/agent-doc/editors/jetbrains/src/main/kotlin/com/github/btakita/agentdoc/TerminalUtil.kt`
- `src/agent-doc/editors/jetbrains/src/test/kotlin/com/github/btakita/agentdoc/EditorCommandStateMachineTest.kt`
- `src/agent-doc/editors/jetbrains/src/test/kotlin/com/github/btakita/agentdoc/TerminalUtilTest.kt`
- `src/agent-doc/agent-doc-orchestration/src/route/dispatch.rs`
- `src/agent-doc/editors/jetbrains/SPEC.md`
- `src/agent-doc/editors/jetbrains/gradle.properties`
- `src/agent-doc/VERSIONS.md`
- root `src/agent-doc` gitlink

Verification:
- `cargo fmt --check`
- `cargo test -p agent-doc-orchestration route_submit_resubmit_proof_line_is_operator_greppable_for_both_harnesses -- --nocapture`
- `./gradlew test --tests com.github.btakita.agentdoc.EditorCommandStateMachineTest --tests com.github.btakita.agentdoc.TerminalUtilTest`
- `make bump-plugin`
- `make check` (`clippy`, 4635 Rust tests, doctests, medium simulator, version sync)
- `./gradlew test`
- `make install`
- `agent-doc plugin install --local jetbrains` for `/home/brian/.config/JetBrains/IntelliJIdea2026.1` and `/home/brian/.local/share/JetBrains/IntelliJIdea2026.1`

CI:
- `src/agent-doc` latest CI for `4ea5f4b9` was `in_progress` when checked.
- root `agent-loop` has no workflow named `CI`.

Commits:
- `src/agent-doc`: `4ea5f4b9 fix jetbrains clear preempts stalled dispatch` pushed to `origin/main`.
- root: `3839e1512 agent-doc: update jetbrains clear preemption` pushed to `origin/main`.

Install / activation:
- CLI/cdylib installed with `make install`; 2 controllers were marked for idle-boundary recycle.
- JetBrains plugin `0.2.169` is installed. Restart IntelliJ IDEA 2026.1 to activate the plugin bytecode.
- parent commit `c815ce6a6` (`agent-doc: update vscode run clear parity`) pushed to `btakita/agent-loop main`
- `src/agent-doc` commit `d71577d4` (`align vscode run clear behavior`) pushed to `btakita/agent-doc main`
- `gh run list --workflow CI --limit 1`: latest CI run at `2026-06-17T17:40:27Z` was completed/success for `fix jetbrains clear preempts stalled dispatch`
- `make check`: passed clippy, 4,635 nextest tests, doctests, and the ignored medium seed corpus check
- `git diff --check`: passed
- `agent-doc plugin install --local vscode`: installed `agent-doc-0.2.28.vsix` via `codium`
- `npm run package` in `editors/vscode`: built `agent-doc-0.2.28.vsix` with `SPEC.md` and compiled extension output included
- `npm test` in `editors/vscode`: 89 tests passed, including the new `editorCommandState` suite
- `npm install --package-lock-only --ignore-scripts` in `editors/vscode`
- Session status, restart, interrupt-clear, and diagnostics remain binary-owned; the plugin does not use stale local busy/status flags to block clear.
- VS Code Run clicks during an active clear now queue the latest run intent and start only after clear and any chosen clear-refusal recovery action complete.
- VS Code `Clear Session Context` now preempts a still-dispatching Run by aborting that route subprocess, waiting briefly for it to settle, then invoking the normal binary-owned `agent-doc session clear <relative-path>` path.
- Repeated VS Code `Run Agent Doc` clicks for the same document now dedupe behind the first alive plugin-spawned route process.
- parent submodule pointer `src/agent-doc`
- `src/agent-doc/VERSIONS.md`
- `src/agent-doc/editors/vscode/package-lock.json`
- `src/agent-doc/editors/vscode/package.json`
- `src/agent-doc/editors/vscode/README.md`
- `src/agent-doc/editors/vscode/SPEC.md`
- `src/agent-doc/editors/vscode/src/extension.ts`
- `src/agent-doc/editors/vscode/src/editorCommandState.test.ts`
- `src/agent-doc/editors/vscode/src/editorCommandState.ts`

Confidence: high.
Escalation: none.

### Re: VS Code plugin parity with JetBrains Run/Clear — gpt-5

> **Queue prompt:**
>
> :pushpin: VSCode plugin should have the same specs + behavior as the JB plugin.


Implemented and pushed the VS Code parity work for the active queue head.

Summary:
- Added a VS Code per-document Run/Clear state machine with unit tests.
- Wired `Run Agent Doc` to dedupe repeated clicks behind the first alive route process.
- Wired `Clear Session Context` to abort a still-dispatching plugin route process, wait briefly for it to settle, then invoke binary-owned `agent-doc session clear <relative-path>`.
- Wired Run during active Clear to queue and start after clear and selected clear-refusal recovery finish.
- Added `editors/vscode/SPEC.md`, updated the VS Code README/session-command docs, and bumped the local VS Code package to `0.2.28`.

Verification:
- `npm test` in `src/agent-doc/editors/vscode`: 89 passed.
- `npm run package`: built `agent-doc-0.2.28.vsix`.
- `agent-doc plugin install --local vscode`: installed via `codium`.
- `git diff --check`: passed.
- `make check`: passed clippy, 4,635 nextest tests, doctests, and the ignored medium simulator check.
- `gh run list --workflow CI --limit 1`: latest CI was completed/success for the prior pushed `src/agent-doc` commit.

Commits:
- `src/agent-doc` `d71577d4` (`align vscode run clear behavior`) pushed.
- parent `agent-loop` `c815ce6a6` (`agent-doc: update vscode run clear parity`) pushed.

Escalation: none. Confidence: high. I did not perform a live manual VS Code race-click repro; the local VS Code package was built and installed, and the pure state machine plus existing session UI command/refusal coverage pass.

### Re: Why did the observation loop not detect the unsubmitted prompt and not send Return? — gpt-5

> **Queue prompt:**
>
> JB `Run Agent Doc` worked on this document. But on equityfundingsource.md, did not submit:
> ```
> resync: pruned 4 stale session(s)
> [route] target tmux session: 0
> [route] codex ready after 0.2s (1 polls)
> [route] Sent agent-doc /home/brian/work/btakita/agent-loop/tasks/professional/equityfundingsource.md → pane %63
> [route] latency budget exceeded: phase dispatch_start_proof took 10114ms (budget 10000ms, pane=%63, harness=codex, outcome=unproven_but_accepted)
> [route] preserved dispatch-start proof snapshot for tasks/professional/equityfundingsource.md pane %63 harness=codex phase=direct_pane_dispatch_start_unproven capture_len=1174 capture_hash=809fcf302ba3 snapshot_path=/home/brian/work/btakita/agent-loop/.agent-doc/logs/route-submit/1781718619383-direct_pane_dispatch_start_unproven-codex-_63-809fcf302ba3.txt editor_attempt_id=1781718607979-3
> [route] bare codex reopen for tasks/professional/equityfundingsource.md was accepted in pane %63, but no routed submission proof appeared after 10s
> Error: dispatch-only codex reopen for tasks/professional/equityfundingsource.md was accepted in pane %63 via direct_pane_submit (tmux_text_enter), but only pane-input acceptance proof was available after waiting 10s; treating this as not dispatched because no dispatch-start proof was recorded. Restore an idle codex prompt or restart the session and reroute again
> ```
>
> Why did the observation loop not detect the unsubmitted prompt and not send Return?


Root cause: the route had already observed accepted text+`Enter` delivery, but the Codex dispatch-only proof gate still required a stronger dispatch-start signal. When that stronger proof did not arrive within 10s, the code failed with `accepted_without_dispatch_start_proof` instead of treating accepted delivery as a successful dispatch-only reroute. The late-Enter retry path only runs when the same routed prompt is still visibly drafted after the proof wait; this equityfundingsource failure was the accepted-only hard-fail path, so it marked the route blocked rather than sending another Return.

Fix implemented:
- `dispatch_only_dispatch_start_proof_required` now returns false for Codex as well as Claude/OpenCode.
- Codex accepted text+`Enter` delivery now succeeds with `proof=accepted proof_scope=accepted_only` instead of leaving a route-submit block.
- Updated dispatch-only tests from the old Codex-fail-closed expectation to all-harness accepted-delivery coverage.

Changed paths:
- `src/agent-doc/agent-doc-orchestration/src/flow/routed_reopen.rs`
- `src/agent-doc/agent-doc-orchestration/src/route/dispatch_only.rs`
- parent submodule pointer `src/agent-doc`

Verification:
- `cargo test -p agent-doc-orchestration dispatch_only_ -- --nocapture`: focused dispatch-only suite passed.
- `cargo fmt`
- `make check`: passed clippy, 4,635 nextest tests, doctests, and the ignored medium simulator check.
- `make install`: installed `agent-doc v0.34.1` and `libagent_doc-0.34.1.so`; 2 controllers marked for idle-boundary recycle.
- `gh run list --workflow CI --limit 1`: CI for `d00bfa9e` was `in_progress` at `2026-06-17T18:06:45Z`; I did not wait.

Commits:
- `src/agent-doc` `d00bfa9e` (`accept codex dispatch-only enter delivery`) pushed.
- parent `agent-loop` `42a229f84` (`agent-doc: accept codex dispatch-only delivery`) pushed.

Escalation: none. Confidence: high for this policy fix; no live equityfundingsource reroute was forced after install.

### Re: JB File Cache Conflict / editor-active disk writes — gpt-5

> **Queue prompt:**
>
> Still getting JB `File Cache Conflict`. Evaluate all paths in the codebase that can cause an external file write while the plugin is running. Make sure all paths only send the edit through the editor plugin while the editor plugin is running. VSCode should have the same behavior.


This head is already implemented in the current `src/agent-doc` codebase; I did not need another source change in this cycle.

Implemented behavior already present:
- Direct document disk fallback is allowed only when no JetBrains/VS Code IPC listener is active.
- Active-listener failures fail closed with `transport=blocked ... action=refuse_external_disk_write` instead of writing behind the editor and causing File Cache Conflict dialogs.
- The shared rule covers document convergence, plain maintenance writes, queue consume, template apply, compact, pending/review/dedupe/preflight maintenance, and live-prompt auto-recovery paths.
- VS Code is covered by the same no-listener-only disk fallback rule because this is enforced in Rust orchestration before plugin-specific delivery.

Existing implementation proof:
- `src/agent-doc` commit `59d2d9e5` (`block editor-active disk fallbacks`) landed the ban.
- Root pointer commit `f5ed3a04c` landed that submodule update.
- The exchange already recorded the implementation under `### Re: JB File Cache Conflict / external file edits`.

Current verification still passing after later changes:
- `make check`: passed clippy, 4,635 nextest tests, doctests, and the ignored medium simulator check.
- The relevant focused coverage remains in `write::converge::core_tests`: `converge_document_or_disk_blocks_disk_fallback_with_active_listener_without_ack_content`, `converge_or_disk_write_blocks_plain_disk_fallback_with_active_listener_without_ack_content`, and `try_auto_recover_live_prompt_drift_blocks_disk_fallback_with_active_listener_without_ack_content`.
- `make install` installed the current CLI/cdylib and marked 2 controllers for idle-boundary recycle.

Escalation: none. Confidence: high for the repository behavior; I did not run a new live JetBrains File Cache Conflict repro in this cycle.

### Re: agent-doc on monsterrodholders.md keeps on deleting the `agent:queue` element. — gpt-5

> **Queue prompt:**
>
> agent-doc on monsterrodholders.md keeps on deleting the `agent:queue` element.


Fixed and shipped.

The stale-IPC repair path now preserves baseline `agent:queue` prompts when socket ACK content, file-IPC ACK content, or file-read fallback content omits them during `live_prompt_drift_after_preflight`. Those omissions are no longer treated as authoritative user deletions; the path logs `queue_live_deletion_ignored ... reason=unproven_ipc_candidate_queue_deletion` and the disjoint outside-edit forward-merge tolerance now refuses candidates that drop baseline queue prompts. Normal queue consumption and done-id handling remain the deletion authority.

Changed files: `agent-doc-orchestration/src/write.rs`, `agent-doc-orchestration/src/write/ipc.rs`, `VERSIONS.md`, and `specs/07-closeout-commands.md`.

Verification passed: focused regressions for `ipc_live_prompt_drift_content_ours_ignores_unproven_live_queue_deletions`, `preserve_content_ours_over_live_queue_deletions_keeps_baseline_prompts`, and `fintol_queue_deletion_is_not_forward_merged_with_outside_edit`; full `cargo test -p agent-doc-orchestration`; `cargo fmt`; `git diff --check`; full `make check` with 4,636 nextest tests plus doctests and medium simulation; and `make install` installed `agent-doc 0.34.1` plus the FFI library and marked two controllers for idle recycle.

Pushed: `btakita/agent-doc` `a1e139d5` (`fix: ignore unproven live queue deletions`). The parent repo records that submodule SHA at pushed root HEAD `bfb58e854`; that parent commit was produced by a concurrent agent-doc session and includes its session document change along with the gitlink. Agent-doc CI run `27710668979` is in progress; the parent repo has no workflow named `CI`.

### Re: agent-doc on Codex outputs console messages that obscures the TUI ![img_38.png](img_38.png) after a turn after a `/clear`, and after a restart. — gpt-5

> **Queue prompt:**
>
> agent-doc on Codex outputs console messages that obscures the TUI ![img_38.png](img_38.png) after a turn after a `/clear`, and after a restart.


Fixed and shipped.

The screenshot showed routine supervisor stderr painting over the Codex pane: stale-busy reconcile and hot-reload status lines. The existing `agent-doc start` path already redirected supervisor stderr for TUI harnesses, but Codex was incorrectly excluded from that classification. Codex is now classified with OpenCode as a TUI harness, so managed Codex supervisors redirect their own stderr to `.agent-doc/logs/supervisor-stderr.log` after startup instead of printing over the foreground TUI. Claude remains attached to supervisor stderr for its existing operator-prompt contract.

Changed files: `agent-doc-orchestration/src/harness.rs`, `agent-doc-orchestration/src/start/run.rs`, `specs/supervisor.md`, and `VERSIONS.md`.

Verification passed: `cargo fmt`; focused `cargo test -p agent-doc-orchestration is_tui_harness -- --nocapture`; focused `cargo test -p agent-doc-orchestration route_owned_start_status_logs_without_printing_by_default -- --nocapture`; `git diff --check`; full `make check` with 4,636 nextest tests plus doctests and medium simulation; and `make install`, which installed `agent-doc 0.34.1` plus the FFI library and marked two controllers for idle recycle.

Pushed: `btakita/agent-doc` `560f7b49` (`fix: redirect codex supervisor stderr`) and parent gitlink commit `00c63c1a6` (`agent-doc: redirect codex supervisor stderr`), with root HEAD now recording `src/agent-doc` at `560f7b49`. Agent-doc CI run `27711235635` is in progress; the parent repo has no workflow named `CI`.
<!-- agent:boundary:22bc1c70 -->
<!-- /agent:exchange -->

## Queue

<!-- agent:queue priority preset="#spec-test-build-install-commit-push" priority go -->
- ~~agent-doc on monsterrodholders.md keeps on deleting the `agent:queue` element.~~
- ~~agent-doc on Codex outputs console messages that obscures the TUI ![img_38.png](img_38.png) after a turn after a `/clear`, and after a restart.~~
- agent-doc sent a prompt to Codex (does this with other models too) while a task was running ![img_39.png](img_39.png)
- On monsterrodholders.md, after a JB `Clear Session Context` was successful, JB `Run Agent Doc` added a duplicate prompt (one with the relative path and one with the absolute path), and did not submit the prompt ![img_41.png](img_41.png).
- agent-doc is spamming the same prompt as I type into the queue in Codex on monsterrodholders.md ![img_42.png](img_42.png)
- Restart on equityfundingsource.md failed ![img_40.png](img_40.png)
<!-- /agent:queue -->
###
<!--
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
<!-- /agent:backlog -->

## Review
 
<!-- agent:review -->
- [/] [#rtwbcast-verify] [operator-verify] rtwbcast rung 4 final proof remains gated: code is landed in agent-doc cddad87b and headless targeted-delivery proof exists via SimWorld, but agent closeout on 2026-06-16 found only JetBrains live broadcast queue markers and no JetBrains-plus-VS Code live convergence/ack proof. Acceptance still requires opening the same managed doc in JB and VS Code, editing both buffers, and confirming both converge with no clobber and zero File Cache Conflict. Plan: tasks/agent-doc/plan-realtime-editor-watcher.md
<!-- /agent:review -->

<!-- agent:icebox -->
<!-- /agent:icebox -->

## Completed / Reaped

<!-- agent:done archive=tasks/agent-doc-bugs2.done.md -->

<!-- completed work archived in tasks/agent-doc-bugs2.done.md -->
<!-- /agent:done -->
