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

#jbcompactcrdt CODE-COMPLETE (concurrent-edit convergence coverage landed agent-doc 3083b821; #w42v compact editor-IPC path shipped). Operator-drive remainder: lib-install + live Compact Exchange verify. #rtwbcast remains queued (separate multi-editor item).
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7a9b103bd5b68223dac05a4c449ac03899aba53d6022e82100578b5b0-20260616-063307.md`*

Compacted content:
- Archived 12 response topic(s): JB Run Agent Doc still does not work; #jbruncloseoutstate; How can agents think architecturally instead of sprawl code?; 9 more
- Prior summary/context: ❯ *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7a9b103bd5b68223dac...
I mapped the tmux interaction surfaces across the repo. I treated `src/agent-doc` and `src/tmux-router` as the canonical implementation, and treated `src/agent-doc-verify-*`, `src/agent-doc-qchurnfix`, and `src/agent-doc-efsmrgmarkers` as duplicate worktree/checkpoint copies rather than separate owners.

Primary tmux owner surfaces:

- `src/tmux-router/src/tmux.rs`: the low-level tmux wrapper. This is the main command boundary for `tmux` process execution, pane/session/window queries, pane creation, `send-keys`, `capture-pane`, `display-message`, `join-pane`, `break-pane`, `swap-pane`, `kill-pane`, `kill-session`, `set-option`, `resize-pane`, batching, and isolated test servers.
- `src/tmux-router/src/sync.rs`: declarative layout reconciliation. It arranges editor columns into tmux panes, preserves/replaces/stashes panes, equalizes sizes, selects focus, and updates registry state after moves.
- `src/tmux-router/src/registry.rs`: file-to-pane registry and pruning support. It does not own tmux behavior by itself, but it is the durable identity layer used by tmux routing.
- `src/tmux-router/src/pane_policy.rs`: session-boundary guards for pane movement such as `join-pane` and `swap-pane`.
- `src/tmux-router/src/hybrid.rs`: harness-aware tmux fallback input and `pipe-pane` output streaming.
- `src/tmux-router/src/control.rs`: `tmux -C` control-mode client and parser.
- `src/tmux-router/src/main.rs`: standalone `tmux-router` CLI for sync/register/send/capture/events.

Agent-doc tmux adapter and primitives:

- `src/agent-doc/agent-doc-orchestration/src/sessions.rs`: agent-doc's compatibility layer over `tmux-router`. It re-exports `Tmux`/`IsolatedTmux`, reads current session/pane, lists panes, captures pane content with and without ANSI, sends raw keys, and centralizes harness-specific submit profiles for Claude/Codex/OpenCode.
- `src/agent-doc/agent-doc-orchestration/src/env.rs`: protects environment-variable expansion for tmux-delivered commands so secrets are expanded inside the target pane rather than exposed in `send-keys` arguments or scrollback.
- `src/agent-doc/agent-doc-orchestration/src/harness.rs`: defines harness trigger templates and tmux-visible prompt/status parsing assumptions.

Agent lifecycle, routing, and dispatch:

- `src/agent-doc/agent-doc-orchestration/src/start.rs` and `src/agent-doc/agent-doc-orchestration/src/start/run.rs`: start/reuse managed panes, verify tmux availability, bind documents to panes, surface capability proof with `display-message`, and handle stale/duplicate pane recovery.
- `src/agent-doc/agent-doc-orchestration/src/start/idle_watch.rs`, `start/detection.rs`, and `start/decisions.rs`: watch managed pane lifecycle/readiness and decide when auto-trigger or queue work may be injected.
- `src/agent-doc/agent-doc-orchestration/src/supervisor/screen.rs`, `supervisor/ipc.rs`, `supervisor/pty.rs`, `supervisor/in_process.rs`, and `supervisor/resize.rs`: supervisor-side terminal model and IPC/PTY boundary. These reduce reliance on `capture-pane`, but still coordinate with tmux-owned panes.
- `src/agent-doc/agent-doc-orchestration/src/route.rs`: top-level route coordinator. It resolves the project tmux session, focuses/inspects panes, handles startup/busy/blocked actors, captures pane tails for readiness and diagnostics, dispatches reopen commands, and renders operator tmux inspect/kill guidance.
- `src/agent-doc/agent-doc-orchestration/src/route/session_resolution.rs`, `pane_resolution.rs`, `authoritative_actor.rs`, `startup.rs`, `busy_pane.rs`, `dispatch.rs`, `dispatch_only.rs`, and `cycle_ack.rs`: the route submodules that resolve tmux targets, wait for prompt/dispatch proof, and decide whether to submit through supervisor IPC or direct tmux pane input.
- `src/agent-doc/agent-doc-orchestration/src/flow/routed_reopen.rs`: pure route decision state that feeds the tmux/supervisor effects in `route.rs`.
- `src/agent-doc/src/queue_dispatch.rs`: fallback dispatch for queued slash commands. It prefers supervisor IPC and then falls back to the canonical tmux submit path for a live pane.
- `src/agent-doc/src/parallel.rs`: fan-out orchestration that creates panes, sends task launch commands, stashes panes, and can kill spawned panes during cleanup.

Session, operator, and diagnostics commands:

- `src/agent-doc/src/main.rs` and `src/agent-doc/src/commands.rs`: CLI surface for start, route, claim, focus, layout, sync, prompt, session, terminal, turn-status, etc.; many of those commands eventually cross a tmux boundary.
- `src/agent-doc/src/session_cmd.rs`: configured tmux-session binding/migration, including moving agent-doc/stash windows between sessions.
- `src/agent-doc/src/session_actor_cmd.rs`: operator clear/restart/interrupt/status/attach paths. It gathers live pane evidence with tmux, sends clear/restart input when safe, checks foreground/prompt state, and reports recovery guidance.
- `src/agent-doc/src/terminal.rs`: opens/attaches an external terminal to a tmux session, creating the session when needed.
- `src/agent-doc/agent-doc-orchestration/src/prompt.rs`: detects Claude/OpenCode permission prompts by capturing pane content, then answers by sending tmux keys.
- `src/agent-doc/agent-doc-orchestration/src/focus.rs` and `src/agent-doc/src/autoclaim.rs`: focus/select registered panes and keep claim/focus state aligned with the visible tmux pane.
- `src/agent-doc/agent-doc-orchestration/src/turn_status.rs`: sets pane titles/status for active/idle turn state.
- `src/agent-doc/src/skill.rs`: installer/setup path that can apply tmux-related shell integration.

Sync, layout, repair, and resync:

- `src/agent-doc/agent-doc-orchestration/src/sync.rs`: main editor-layout-to-tmux reconciliation command. It wraps tmux-router, runs layout repair, handles safe-passive editor sync, protects active closeout panes, and records sync diagnostics.
- `src/agent-doc/agent-doc-orchestration/src/sync/layout.rs`, `sync/pane_repair.rs`, `sync/registry.rs`, and `sync/safe_passive.rs`: layout normalization, pane repair, registry projection, and safe passive sync policy.
- `src/agent-doc/src/layout.rs`: older/facade layout command path for arranging panes to mirror editor splits.
- `src/agent-doc/agent-doc-orchestration/src/resync.rs`, `resync/prune.rs`, and `resync/stash.rs`: validates session registry rows against live tmux, prunes stale panes, relocates wrong-session panes with `join-pane`, purges dead/non-owned panes, manages stash windows, and closes superseded sessions.
- `src/agent-doc/agent-doc-orchestration/src/admin.rs`, `dashboard.rs`, `preflight.rs`, `preflight/maintenance.rs`, `project_controller/rpc.rs`, `queue_cmd.rs`, `queue_continuation.rs`, and `watch.rs`: these mostly consume or project tmux-backed actor/session state, gate dispatch, or surface stale supervisor/install/layout diagnostics rather than owning tmux commands directly.

Editor integrations:

- `src/agent-doc/editors/jetbrains/src/main/resources/META-INF/plugin.xml`: registers `Run Agent Doc`, `Claim for Tmux Pane`, `Force Claim for Tmux Pane`, `Sync Tmux Layout`, and `Load Tmux Window` actions.
- `src/agent-doc/editors/jetbrains/src/main/kotlin/com/github/btakita/agentdoc/SubmitAction.kt` and `TerminalUtil.kt`: JetBrains `Run Agent Doc` process path. The plugin invokes the `agent-doc route ...` CLI, records route attempt ledgers, classifies busy/queued/failure output, and presents tmux/session status to the user.
- `ClaimAction.kt`, `SyncLayoutAction.kt`, `LoadTmuxWindowAction.kt`, `EditorTabSyncListener.kt`, `RefreshEnvironmentAction.kt`, `PluginLifecycleListener.kt`, and `NativeLib.kt`: claim/sync/load/bind flows from the IDE side. `RefreshEnvironmentAction` and `PluginLifecycleListener` directly call tmux for session discovery; the other actions mostly call the agent-doc binary/FFI paths that perform tmux work.
- `src/agent-doc/editors/vscode/src/extension.ts`, `tabSync.ts`, `popupMenu.ts`, and their tests: VS Code-facing claim/sync/cross-session status and UI presentation around the same agent-doc tmux commands.

Tests, specs, and CI:

- `src/agent-doc/Makefile`: `make tmux-ci` runs ignored live tmux tests with `TMUX_TEST_THREADS`.
- `src/agent-doc/.github/workflows/ci.yml` and root `.github/workflows/agent-doc.yml`: install tmux and run the live tmux CI leg.
- `src/agent-doc/tests/*`, many inline `#[ignore = "live tmux integration test"]` tests, and `src/tmux-router` tests: use `IsolatedTmux` to exercise real tmux servers without touching the user's default server.
- `src/agent-doc/specs/07-session-tmux-commands.md`, `specs/multiplexer-backend.md`, `specs/terminal-backend.md`, `specs/supervisor.md`, `specs/08-session-routing.md`, and `specs/12-deterministic-simulation.md`: the behavioral contracts for tmux routing, submit modes, layout sync, supervisor migration, and what must remain covered by real tmux tests versus SimWorld.

Root and project-local helper scripts:

- `bin/agent-doc-start` and `bin/agent-doc-route`: legacy/root shell wrappers that check `TMUX`/`TMUX_PANE`, create/reuse a `claude` tmux session, send `/agent-doc <file>` with `tmux send-keys`, and register panes.
- `bin/install.sh`: installs/checks tmux for the workspace bootstrap path.
- `bin/tmux-setup.sh`: personal/dev workspace layout script that renames windows, splits panes, selects panes, and launches sample `agent-doc start` commands.
- App/submodule dev helpers: `src/briantakita.me-dev/app/-briantakita.me-dev-bin/bin/tmux-dev.sh`, `src/brookebrodack-dev/app/-brookebrodack-dev-bin/bin/tmux-dev.sh`, `src/censible-core/bin/tmux-dev.sh`, `src/censible-core/bin/tmux-rename-window.sh`, `src/cogov-dev/app/-dev-bin/bin/tmux-dev.sh`, `src/cogov-dev/app/-dev-bin/bin/tmux-rename-window.sh`, `src/ctx-core-dev/bin/tmux-dev.sh`, `src/ctx-core-dev/bin/tmux-rename-window.sh`, `src/drmanny-dev/app/-drmanny-dev-bin/bin/tmux-dev.sh`, `src/herbaliciousbliss-dev/app/-herbaliciousbliss-dev-bin/bin/tmux-dev.sh`, `src/prescience-notes-dev/lib/dev-bin--prescience-notes/bin/tmux-dev.sh`, and `src/rappstack-dev/lib/-rappstack-dev-bin/bin/tmux-dev.sh`. These are project-local layout/startup helpers, not part of agent-doc routing.
- `src/ctx-core-dev/lib/rollup/bin/rollup-cmd.mjs`: generates tmux sessions/windows/panes for rollup command orchestration.
- Package scripts in the related app submodules expose those `tmux-dev` / `tmux-rename-window` helpers.

Low-level adjacent surface:

- `src/agent-kit/src/hooks.rs` and `src/agent-kit/SPEC.md`: define hook vocabulary including tmux layout-change events. This is an integration vocabulary surface, not the tmux command owner.

So the ownership boundary is mostly clean: `tmux-router` owns raw tmux mechanics, `agent-doc-orchestration::sessions` owns the agent-doc adapter/submission contract, route/start/sync/resync/session commands own policy and effects, and editor plugins should generally call agent-doc CLI/FFI rather than recreating tmux behavior. The exceptions are editor startup/session discovery calls and the root/dev helper scripts.

Proof for this cycle:
- Changed paths: this session document only.
- Commands: `agent-doc preflight ...`; `agent-doc plan ...`; `tsift status`; `tsift index --workspace .`; targeted `rg` searches for `tmux`, `TMUX`, `Command::new("tmux")`, `send-keys`, `capture-pane`, `display-message`, `list-panes`, `split-window`, `join-pane`, `kill-pane`, `pipe-pane`, and `attach_control_mode`; exact helper list via `rg --files src | rg '(^|/)bin/tmux[^/]*\.sh$' | sort`.
- Verification: source/search documentation only; no source code changed, so I did not run the build/test suite.
- Confidence: high for canonical agent-doc/tmux-router/editor surfaces; medium for every non-agent-doc submodule because some are independent app repos with incidental tmux dev helpers rather than one shared tmux subsystem.
- Escalation: none.

### Re: restart mangles the console — gpt-5

Fixed the restart console-mangling path in `src/agent-doc`.

What changed:
- `agent-doc run` now redirects routine run/diff/commit stderr to `.agent-doc/logs/run-stderr.log` when it is invoked with terminal stderr inside a tmux pane owned by a Codex/OpenCode parent harness.
- The redirect is skipped for dry-run, non-terminal stderr, non-managed harnesses, and verbose input diagnostics (`AGENT_DOC_TMUX_INPUT_DIAG` / `AGENT_DOC_DEBUG_STDIN`), so foreground diagnostics still work when explicitly requested.
- Stderr is restored before the CLI returns, so real terminal errors still surface normally.
- Heartbeat/progress state still persists into cycle state while the foreground TUI stays clean.

Regression coverage:
- Added `run_heartbeats_redirect_stderr_under_managed_tui_but_persist_progress`.
- Updated the existing visible-heartbeat test to opt into foreground diagnostics explicitly.
- Updated README/SPEC/backend/core-command docs for the new managed-TUI stderr contract.

Verification:
- `cargo test --test run_integration run_heartbeats` passed.
- `cargo test --test run_integration codex_` passed.
- `cargo test --test run_integration run_auto_queue` passed.
- `make check` passed locally.
- `make install` completed and installed `agent-doc 0.34.1` plus `libagent_doc-0.34.1.so`.
- GitHub CI run `27599686384` passed after push, including hosted `make check` and `make tmux-ci`.

Commits pushed:
- `src/agent-doc`: `4ba0ac2d fix: redirect run diagnostics for managed TUIs`
- superproject: `04b461e52 chore: update agent-doc submodule`
- Confidence: high for the snapshot/logging and split-submit behavior. If Enter still misses in the live restart path, the next `ops.log` plus the referenced snapshot file should show whether the composer kept the draft, consumed it without hook proof, or failed capture.
- Escalation: none.
- Commits pushed: agent-doc `dc641c25 fix: capture route submit pane snapshots`; superproject `1c47aa09a chore: update agent-doc submodule`.
- CI: agent-doc CI run `27601595380` is in progress after push; it was not red when checked. The parent repo has no workflow named `CI`.
- Local install completed: `make install` installed `agent-doc 0.34.1` and `libagent_doc-0.34.1.so`, and marked 2 controllers for idle recycle.
- Full local verification passed: `make check`.
- Targeted tests passed: `route_pane_snapshot_preserves_redacted_terminal_capture`, `submit_profiles_keep_harness_submit_policy_in_one_place`, `route_submit_observation_marks*`, and `pending_payload_enter_resubmit_is_scoped_and_one_shot`.
- Changed paths: `VERSIONS.md`, `agent-doc-orchestration/src/sessions.rs`, `agent-doc-orchestration/src/route.rs`, `agent-doc-orchestration/src/route/dispatch.rs`, `specs/07-session-tmux-commands.md`.
- Updated the session/tmux command spec and `VERSIONS.md` for the new forensic contract.
- `ops.log` now gets `route_pane_snapshot ... snapshot_path=...` for those captures, so the next repro has terminal evidence tied to the route phase.
- Route now preserves redacted pane-output snapshots under `.agent-doc/logs/route-submit/` when it sees a pre-existing drafted trigger, a still-visible trigger after submit/resubmit polling, or accepted pane delivery without required dispatch-start proof.
- The tmux submit profile now sends literal text and a named `Enter` as two separate `tmux send-keys` calls, so a successful draft write no longer hides a missed Enter event.

I left the pre-existing unstaged PCP/control-plane edits in `src/agent-doc` alone: `agent-doc-orchestration/src/start/decisions.rs` and `specs/08b-single-process-control-plane.md`.

### Re: Enter submit snapshots — gpt-5

> **Queue prompt:**
>
> :pushpin: The Enter still was not applied to the prompt after the restart. As I was typing, `/clear` was accumulating. It seems the separate Enter does not work. Can you have the supervisor or the PCP take snapshots of the terminal oput so you can investigate what happens? Perhaps to self-correct and log what the issues are?


Implemented this in `src/agent-doc`.

What changed:
❯ - The tmux submit profile now sends literal text and a named `Enter` as two separate `tmux send-keys` calls, so a successful draft write no longer hides a missed Enter event.
❯ - Route now preserves redacted pane-output snapshots under `.agent-doc/logs/route-submit/` when it sees a pre-existing drafted trigger, a still-visible trigger after submit/resubmit polling, or accepted pane delivery without required dispatch-start proof.
❯ - `ops.log` now gets `route_pane_snapshot ... snapshot_path=...` for those captures, so the next repro has terminal evidence tied to the route phase.
❯ - Updated the session/tmux command spec and `VERSIONS.md` for the new forensic contract.

Proof:
❯ - Changed paths: `VERSIONS.md`, `agent-doc-orchestration/src/sessions.rs`, `agent-doc-orchestration/src/route.rs`, `agent-doc-orchestration/src/route/dispatch.rs`, `specs/07-session-tmux-commands.md`.
❯ - Targeted tests passed: `route_pane_snapshot_preserves_redacted_terminal_capture`, `submit_profiles_keep_harness_submit_policy_in_one_place`, `route_submit_observation_marks*`, and `pending_payload_enter_resubmit_is_scoped_and_one_shot`.
❯ - Full local verification passed: `make check`.
❯ - Local install completed: `make install` installed `agent-doc 0.34.1` and `libagent_doc-0.34.1.so`, and marked 2 controllers for idle recycle.
❯ - CI: agent-doc CI run `27601595380` is in progress after push; it was not red when checked. The parent repo has no workflow named `CI`.
❯ - Commits pushed: agent-doc `dc641c25 fix: capture route submit pane snapshots`; superproject `1c47aa09a chore: update agent-doc submodule`.
❯ - Escalation: none.
❯ - Confidence: high for the snapshot/logging and split-submit behavior. If Enter still misses in the live restart path, the next `ops.log` plus the referenced snapshot file should show whether the composer kept the draft, consumed it without hook proof, or failed capture.

I left the unrelated pre-existing PCP handoff edits in `src/agent-doc` unstaged: `agent-doc-orchestration/src/start/decisions.rs` and `specs/08b-single-process-control-plane.md`.

### Re: Add State Machine architecture tasks — gpt-5

Added six State Machine / architecture tasks as stable backlog items at the tail, each marked with `:inbox_tray:` so `agent:queue` sync can materialize them at the end of the live queue:

- `#smcloseoutdecision`: implement the full typed `CloseoutRecoveryDecision` boundary from `tasks/agent-doc/plan-run-agent-doc-closeout-recovery-state-machine.md`.
- `#smcloseoutevidence`: centralize recovery evidence gathering behind one typed API.
- `#smrouteconsume`: make route and JB `Run Agent Doc` consume typed closeout recovery decisions instead of parsing low-level capture errors.
- `#smrecoverymutate`: unify replay, stale-capture retirement, reset-from-visible, and safe queue-only replay through one mutation primitive.
- `#smtransitiontests`: cover the full closeout recovery transition table with pure decision, SimWorld, and property-style tests.
- `#archfirstgate`: apply `runbooks/architecture-first.md` as the closeout gate for these state-machine refactors.

Proof:

- Source context read: archived architecture discussion, `tasks/agent-doc/plan-run-agent-doc-closeout-recovery-state-machine.md`, and `runbooks/architecture-first.md`.
- Mutation path: `agent-doc finalize --pending-add-back ...` with explicit ids and enqueue markers.
- No source code changed in this cycle.
<!-- agent:boundary:301dc14c -->
<!-- /agent:exchange -->

## Queue

<!-- agent:queue priority preset="#spec-test-build-install-commit-push" priority go -->
- ~~:pushpin: The Enter still was not applied to the prompt after the restart. As I was typing, `/clear` was accumulating. It seems the separate Enter does not work. Can you have the supervisor or the PCP take snapshots of the terminal oput so you can investigate what happens? Perhaps to self-correct and log what the issues are?~~
- [#cwsp]
- :pushpin: What are #next-steps to refactor State Machines or other architecture to fix reoccurring classes of bugs?
- :pushpin: Add the tasks for the State Machines and architecture to the end of `agent:queue`.
---
:pushpin: After restarting a session in the terminal, JB `Run Agent Doc` errors. It should not error.
```
resync: pruned 3 stale session(s)
[route] target tmux session: 0
Error: project controller command `dispatch` failed: dispatch blocked for tasks/agent-doc/agent-doc-bugs2.md: failed_stage=queue_paused reason=#qchurn no-op churn: go-mode re-injecting :pushpin: [#qchurn] each idle boundary; all 16 heads are clean-session/operator-verify and undrainable under stale host supervisor pid 2715614; zero drainable work. Pausing to stop the flood until restart onto fresh binary. receipt_id=3649 blocked_head_bytes=132 blocked_head_sha256=ae121dad65e5b00a6c504db706985aee4e8d6ed00988ba4244f51196952739d7 trigger_bytes=44 trigger_sha256=a55a6572fd4dc952bc476bd516d3a7801b1e63409715f063b4b7351ea112ade5
```
---
- do [#lazilyqrestore]
- do [#recyclerestart-verify]
- do [#rtwbcast-verify]
- do [#c2b6]
- do [#1j8q]
- do [#anw0verify]
- do [#qflood2]
- do [#qt32]
- do [#fcc0-live-verify]
- do [#smcloseoutdecision]
- do [#smcloseoutevidence]
- do [#smrouteconsume]
- do [#smrecoverymutate]
- do [#smtransitiontests]
- do [#archfirstgate]
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
- ~~This session's console stdout is bleeding in front of the opencode tui. ![img_16.png](img_16.png)~~
---
The last run had a code fence stripped away by agent-doc.
#spec-test-build-install-commit-push
---
What are #next-steps to fix bugs?
-->

## Backlog

<!-- agent:backlog priority queue -->
- [ ] [#cwsp] [operator-verify] [recommended] If queue editing still interrupts an active Codex owner after installing `src/agent-doc` 2464ba38 and recycling the live supervisor onto that build, add a busy-owner queue-edit suppression guard: queue edits during an active cycle must stay as document state until current closeout commits, must not dispatch into the busy owner pane, and must not let a pre-finalize assistant response be reclassified as a user prompt.
- [ ] [#lazilyqrestore] [operator-verify] Restore/account for lost queue content in `tasks/software/lazily-rs.md`: that document is foreign-owned by another pane, so capture the intended lost queue heads from the owning lazily-rs session/doc history/snapshots/sidecars, restore them as backlog items there, or record explicit user-removal proof. Related #queue-clear-unrun-items/#manual-queue-head-loss/#queue-user-edit-overwrite.
- [ ] [#recyclerestart-verify] [operator-verify] #recyclerestart live proof + promotion: Q1 (recycle ENOENT / restart) + Q2 (FFI self-heal sync guard, supersede-newer-generation) are committed (agent-doc 0b31e90c, fa6a230a) but NOT promoted onto the live stale supervisor — the img_23 stall is the pre-fix binary. Recycle/restart the supervisor onto the new build (agent-doc admin recycle --all-projects, or restart), then confirm: kill-pane + JB Sync Tmux Layout no longer reports Sync deferred, and a real idle/next-queue recycle restarts/re-clears the agent session. Cannot be agent-done. Plan: tasks/agent-doc/plan-pcp-recycle-no-restart-sync-block.md
- [ ] [#rtwbcast-verify] [operator-verify] rtwbcast rung 4 final proof — two-live-IDE CRDT broadcast convergence: with the installed build, edit the same managed doc in JB and VS Code at once and confirm both editor buffers receive the broadcast patch and converge (no clobber). Implementation landed agent-doc cddad87b (id/deliver/merge, SimWorld-covered, CI green). Cannot be agent-done — needs two live editors. Plan: tasks/agent-doc/plan-realtime-editor-watcher.md
- [ ] [#c2b6] [operator-verify] compactdrift live JB Compact Exchange re-verify: with a freshly make-install-ed agent-doc (installed cdylib currently predates aafa6510), run JB Compact Exchange on a doc whose HEAD holds a finalized Re: response and confirm it compacts WITHOUT the "refusing to auto-adopt committed historical response patchback / out-of-band typed_component_drift" bail. Any persistent failure from a NON-wedged session files against stuckhandoff2, not here. Code complete + committed agent-doc bca26cfd; 108 commit_ tests green this cycle.
- [ ] [#1j8q] [operator-verify] #suprecyclelock live proof: drive a real next_queue_item supervisor self-recycle (go-mode queue drain over a freshly-installed binary) while a sibling/concurrent launcher holds the shared project-root controller-launch.lock; confirm the execve'd start no longer surfaces "controller launch already in progress ... (os error 11)" on the pane and ops.log shows the contended waiter adopting the published controller instead. Fixed in agent-doc-orchestration project_controller.rs LaunchLock::acquire_blocking + connect_or_launch.
- [ ] [#anw0verify] [operator-verify] #anw0 redirect live proof: install the new agent-doc binary (cargo build --release + cargo install + lib-install) then drive a real recycle / restart-supervisor race so a dispatcher holding a superseded generation hits the new path. Confirm ops.log shows dispatch_stale_generation_redirect prior_generation=N next_generation=N+1 then dispatch_retry_after_stale_generation and a clean re-dispatch (no terminal generation-N-is-closed). Code+SimWorld landed agent-doc 21a70cdd / superproject 412355fa3. Cannot be agent-done: needs a live editor + real recycle.
- [ ] [#qflood2] [operator-verify] qflood regression: JB Run Agent Doc on efs coalesced as exit-1 (dual-drain owner, gen 66 already in flight). Needs live multi-process verify; if it still collides, coalesce should report deduped-success not exit-1. Plan: tasks/agent-doc/plan-queue-dispatch-flood.md
- [ ] [#qt32] [operator-verify] #lvbremain Operator live zero-drift PROOF of the 08b supervisor-in-process cutover (cannot be agent-done — needs a live editor). Restart a session, drive one edit-during-finalize cycle, agent reads ops.log for write_authority action=routed + plugin_watch_readonly + supervisor_host_gate hosting=in-process + ZERO drift lines. [recommended]
- [ ] [#fcc0-live-verify] [operator-verify] #fcc0 live JB 0-dialog proof: with the current agent-doc binary/cdylib installed and the route-owned supervisor recycled onto it, drive a live JetBrains queue/template/compact write path and confirm ops.log shows editor IPC convergence (for example queue_consume_writeback ... transport=editor_ipc) and zero JetBrains File Cache Conflict dialogs. Cannot be agent-done; requires live IDE observation.
- [ ] [#smcloseoutdecision] :inbox_tray: Implement the full typed CloseoutRecoveryDecision boundary for closeout recovery across route, repair, session-check, and write/commit. Include AlreadyCommitted, ReplaySafe, RetireStaleCapture, ResetSidecarsFromVisible, QueuePromptForAfterCloseout, and Blocked outcomes. Reference tasks/agent-doc/plan-run-agent-doc-closeout-recovery-state-machine.md.
- [ ] [#smcloseoutevidence] :inbox_tray: Centralize closeout recovery evidence gathering behind one typed API: visible markdown hash, snapshot hash, active cycle phase, capture state, response-body/supersession proof, queue-only drift proof, editor IPC freshness, and controller/supervisor binary freshness.
- [ ] [#smrouteconsume] :inbox_tray: Move route and JB Run Agent Doc recovery paths to consume CloseoutRecoveryDecision only; remove low-level string parsing/rephrasing of captured-response baseline errors at route edges.
- [ ] [#smrecoverymutate] :inbox_tray: Unify recovery mutations through one replay/sidecar primitive so reset --from-current --preserve-session, stale-capture retirement, reset-from-visible, and safe queue-only replay cannot diverge.
- [ ] [#smtransitiontests] :inbox_tray: Add SimWorld/property/pure-decision coverage for the full closeout recovery transition table: queue edits around capture/write fragmentation, compaction, stale snapshots, stale ack-content, and JB Run Agent Doc during open closeout with and without prompt context.
- [ ] [#archfirstgate] :inbox_tray: Apply runbooks/architecture-first.md as the closeout gate for agent-doc state-machine refactors: each implementation must state invariant, policy owner, transition rows covered by tests, edited surfaces, and explicit out-of-scope/operator-verify work.
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
