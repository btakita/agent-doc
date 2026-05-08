---
agent_doc_session: b26b9957-8de6-4eae-bb98-5d7fe4d6b781
agent_doc_format: template
agent_doc_write: crdt
agent: codex
claude_args: --dangerously-skip-permissions
codex_args: -s danger-full-access
claude_model: claude-opus-4-6
prompt_presets:
  '#agent-doc-bug': Please create a plan for agent-doc to fix this issue. Add to the backlog of agent-loop/tasks/agent-doc/agent-doc-bugs2.md
  '#code-review': Please review agent-doc codebase. Inspect the architecture and look for edge cases. Isolate concurrency issues and other workflow bugs. Let's find root causes to fix. #follow-up-backlog
  '#commit-push': commit + push all uncommitted files
  '#follow-up-backlog': Any follow-up items to place in the backlog?
  '#next-steps': #follow-up-backlog Please ensure the backlog is in priority order. Print the item id of the top of the backlog.
  "#spec-test-build-install-commit-push": update spec + tests. build + install for local testing. commit + push
  "#spec-test-commit-push": update spec + tests. commit + push
---

## Status

<!-- agent:status patch=replace -->
<!-- normalization -->
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7a9b103bd5b68223dac05a4c449ac03899aba53d6022e82100578b5b0-20260508-082333.md`*

Active backlog:
- [ ] [#closeguard] Implement tasks/agent-doc/plan-closeout-active-prompt-guard.md: fail closeout when a committed response is followed by a newer unresolved prompt-bearing exchange line or when the matching pending item has a malformed checklist prefix.
❯ The test suite is taking a long time to complete. Please increase the parallelization. Apply other safe ways to improve test suite performance.
❯ Why are the "the expected ignored debounce tests" ignored? If these tests should be passing, please fix.
❯ spec-test-build-install-commit-push
### Re: debounce tests and test-suite parallelization — gpt-5

Fixed and pushed.

The ignored debounce tests were old gap-plan placeholders, not tests that should remain skipped. One was waiting on private status-file path access, and two assumed a direct preflight execution seam plus new frontmatter wiring. I converted them into passing executable coverage:

- `test_status_file_write_includes_current_timestamp` now verifies `set_status()` through the public API and the documented `.agent-doc/status` output.
- `test_preflight_timing_1500ms_is_configurable` now enforces that preflight reads `agent_doc_debounce` and passes `debounce_ms` to the typing-indicator check instead of the stale hardcoded `1500`.
- `test_preflight_3s_timeout_is_sufficient_for_debounce` now enforces the existing dynamic max-wait behavior for debounce values above 3s.

The actual bug was in `src/preflight.rs`: preflight already read `agent_doc_debounce` for the mtime-settling path, but the cross-process typing-indicator path still used `1500`. That now uses the configured `debounce_ms` too.

For suite performance, `make test` now prefers `cargo-nextest` when it is installed, then runs doctests separately. If `cargo-nextest` is unavailable, it falls back to Cargo's own test runner with explicit test threads. I avoided keeping the custom direct executable runner because it did not preserve Cargo's integration-test environment such as `CARGO_BIN_EXE_agent-doc`, which made it unsafe for this suite.

Verification completed:

- `cargo test --test debounce_gaps_test_plan -- --include-ignored` passed: 15 passed, 0 ignored.
- `make check` passed.
- `make install` completed and `agent-doc --version` reports `agent-doc 0.33.16`.

Commits pushed:

- `src/agent-doc` branch `20260422/codex-agent-backend`: `a5b6b6e` (`fix: wire configurable preflight debounce`)
- root `agent-loop` branch `main`: `3986b066e` (`chore: update agent-doc submodule`)

I left the pre-existing unrelated root worktree changes alone; only the `src/agent-doc` code changes and the root submodule pointer were committed manually.
❯ We need a way to have extreme edge case coverage and fast tests. Are any testing and architectural principles from Tiger Beetle useful for agent-doc?
### Re: TigerBeetle-style testing for agent-doc — gpt-5

Yes. The most useful TigerBeetle idea for `agent-doc` is not a specific implementation detail; it is the testing posture: make the workflow a small deterministic state machine, attack it with generated edge cases, and treat every failure as a reproducible seed.

The best fit for `agent-doc` would be:

- Deterministic simulation around the response cycle. Model the document, baseline, snapshot, git index, pending list, claims, controller/session state, and assistant response as explicit state. Then generate sequences like user edit, preflight, plan, write, commit, crash, repair, new prompt, pending mutation, stale actor, and route retry.
- Strong invariants instead of example-only tests. Examples: a prompt-bearing exchange line must receive exactly one committed response; finalize must fail if a newer unresolved prompt appears; pending `do #id` must end in done/gated/open-with-reason; session docs must not leak into manual repo commits; recovery must not synthesize duplicate assistant patchbacks.
- Fault injection at every boundary. Kill or error after snapshot creation, after document write, before commit, after staging, during session-check, during controller RPC, and during route/start handoff. Those are where this system actually gets weird.
- Seeded, minimized repros. Every generated failure should print the random seed, command trace, document tail, git state summary, and expected vs actual invariant. That gives us small bug reports instead of one-off stories.
- A fast pure core. Keep parsing, diff classification, planning, pending transitions, and merge decisions mostly pure and tested without shelling out. Reserve slower integration tests for git/filesystem/tmux/controller boundaries.
- Append-only facts plus derived projections where possible. This matches the controller direction: record durable lifecycle facts, derive `sessions.json`/status/projections, and test projection drift as an invariant.

The TigerBeetle pieces I would avoid copying literally are the storage-engine constraints and extreme static-memory discipline. `agent-doc` is a CLI/workflow coordinator, not a database. The useful transfer is architectural: reduce hidden ambient state, make state transitions explicit, make time/concurrency/failure injectable, and run huge numbers of deterministic schedules quickly.

A practical shape would be a `SimWorld` test harness with fake filesystem/git/clock/controller adapters, plus a command DSL such as `EditDoc`, `Preflight`, `Plan`, `Finalize`, `CrashAt`, `Repair`, `RoutePrompt`, and `SessionCheck`. CI can run a short fixed-seed corpus plus a small randomized budget; nightly or local stress can run larger seed ranges. When a bug is found, promote that seed into the corpus.

I added a recommended backlog item for a concrete plan, because this is worth designing deliberately before scattering property tests across the current suite.
❯ do [#tigertest]
### Re: #tigertest — gpt-5

Created and committed the plan: `tasks/agent-doc/plan-tigerbeetle-style-simulation-testing.md`.

The plan defines a TigerBeetle-inspired deterministic simulation/property-test layer for `agent-doc`: a pure `SimWorld`, an explicit command DSL, closeout and session invariants, seeded replay/minimization, fault injection, and split fast/medium/stress test budgets. It scopes the first implementation to closeout/template/pending/snapshot failures, then extends to route/controller schedules once the session actor architecture stabilizes.

I committed the non-session plan file separately as `e0a784d25` (`docs: plan deterministic simulation testing`). No code or test suite changed, so I did not run the full Rust verification suite for this planning-only item.
❯ #next-steps
### Re: next steps — gpt-5

Top backlog item: `#closeguard`.

I kept `#closeguard` first because it protects the core closeout invariant: a session must not close successfully while a newer prompt-bearing exchange line or malformed matching backlog item remains unresolved.

I added one recommended follow-up after it: `#tigersim`, to implement Phase 1-2 from `tasks/agent-doc/plan-tigerbeetle-style-simulation-testing.md`. That keeps the TigerBeetle-style simulator work available without putting it ahead of the immediate closeout guard bug.
❯ do [#closeguard]. spec-test-build-install-commit-push
### Re: #closeguard — gpt-5

Implemented, verified, installed, committed, and pushed.

What changed:
- Added detection for malformed tracked backlog/icebox checklist lines such as `_- [ ] [#id] ...`, so they can no longer disappear from pending guards just because the normal pending parser cannot parse them.
- Wired that detection into `session-check`, where malformed live tracked items now fail closeout with the exact id and line text.
- Added strict pre-write/pre-commit finalize checks so a response that appears to complete a malformed tracked item fails before the response is written.
- Updated the closeout command spec and added unit/integration regressions for the malformed pending-item closeout path.

Verification:
- `cargo test detects_malformed_tracked_checklist_lines`
- `cargo test session_check_blocks_malformed_tracked_item_before_done_guard`
- `cargo test --test finalize_integration finalize_fails_before_write_when_completed_pending_line_is_malformed`
- `cargo build`
- `make check`
- `make install`
- `agent-doc --version` -> `agent-doc 0.33.16`

Commit / push:
- `src/agent-doc` branch `20260422/codex-agent-backend`: `e840cd3` (`fix: fail closeout on malformed pending items`)
- root `agent-loop` branch `main`: `53d6a1f72` (`chore: update agent-doc submodule`)

I left the pre-existing unrelated root workspace changes untouched. No actionable follow-up items came out of this implementation.
<!-- agent:boundary:2d4fdf25 -->
<!-- /agent:exchange -->
###

<!--
"❯ Commit / push:" should not have the `❯ ` prefix since this was the agent response. #agent-doc-bug
---
Please optimize the AGENTS.md + SKILL.md to reduce context usage yet maintain effectiveness for Claude Code + Codex
---
Does the supervisor actor ever block on `Run Agent Doc`? I noticed that /home/brian/work/btakita/agent-loop/tasks/software/tsift did not respond to `Run Agent Doc` in a previous turn. It seems to run ok now but I'm concerned that `Run Agent Doc` was non-responsive.
---
"Press Enter to restart, or 'q' to exit."
-->

## Pending / Not Built

<!-- agent:backlog -->

- [ ] [#tigersim] [recommended] Implement Phase 1-2 from tasks/agent-doc/plan-tigerbeetle-style-simulation-testing.md: inventory pure closeout seams, build the first SimWorld closeout simulator, and promote fixed seeds for recent closeout/pending/snapshot bugs.
<!-- /agent:backlog -->

<!-- agent:icebox -->
<!-- /agent:icebox -->

## Completed / Reaped

<!-- agent:pending-done -->
- 2026-05-07 [#backlogreap] [recommended] Make completed/reaped backlog items visible in the session document or closeout response so items like #commenttail, #tmuxcoord, and #coordspike do not look silently removed after --pending-done.
- 2026-05-07 [#sqliterepros] [recommended] Before prototyping .agent-doc/state.db, capture 2-3 current agent-doc failures where cross-file authority drift or non-atomic state writes would have been fixed by one transactional store rather than by the single-owner actor/supervisor model alone.
- 2026-05-07 [#sqliteeval] [recommended] Review plan tasks/agent-doc/plan-sqlite-session-store-concurrency.md against the single-owner actor rollout and decide whether to prototype .agent-doc/state.db as the authoritative session store.
- 2026-05-07 [#pcperf] [recommended] Add sync/route latency instrumentation and budgets to the Project Controller rollout: measure prune/proof/router/dispatch-ack phases, keep no-op safe-passive sync on the fast path, and surface when a 10s dispatch-proof wait is the reason a pane appears non-responsive.
- 2026-05-07 [#pcctrl] Implement Phase A from tasks/agent-doc/plan-project-controller-session-actor-mvp.md: add the project-local controller shell, singleton socket/launch lock, lazy connect-or-launch helper, controller status command, and bootstrap epoch/launch-mode persistence without changing route/start behavior yet.
- 2026-05-08 [#pcstore] Implement Phase B from tasks/agent-doc/plan-project-controller-session-actor-mvp.md: move actor-record load/store behind controller commands, write SQLite first, emit session-actors.json and sessions.json as projections, and add projection drift diagnostics.
- 2026-05-08 [#pcsuper] Implement Phase C from tasks/agent-doc/plan-project-controller-session-actor-mvp.md: make Harness Supervisor startup register with the controller and report ready/busy/waiting-input/blocked/closed lifecycle facts through controller-owned actor transitions.
- 2026-05-08 [#pcroutes] Implement Phase D from tasks/agent-doc/plan-project-controller-session-actor-mvp.md: convert start, route, and sync to read/dispatch through controller authority and keep tmux-router as layout reconciliation only.
- 2026-05-08 [#pcops] Implement Phase E from tasks/agent-doc/plan-project-controller-session-actor-mvp.md: wire status/history/attach/restart/clear/doctor through controller clients, keep destructive recovery behind doctor --repair, and expose stage-specific editor diagnostics.
- 2026-05-08 [#ctrlreload] [recommended] Add controller binary-generation/version invalidation so agent-doc restarts a stale project controller after local install/update instead of surfacing unknown controller command errors for newly-added controller RPCs.
- 2026-05-08 [#ctrlsock] Implement tasks/agent-doc/plan-controller-stalled-client-start-hang.md: make project controller RPCs robust to stalled clients so a stuck sync/no-autostart or closed prior pane cannot make agent-doc start/session status hang; fail closed with stale-pane diagnostics instead.
- 2026-05-08 [#tigertest] [recommended] Create a TigerBeetle-inspired deterministic simulation/property-test plan for agent-doc: model session-cycle state, generate concurrent document edits and crash/fault schedules, assert closeout/pending/git invariants, and keep most cases in fast pure tests.
- 2026-05-08 [#closeguard] Implement tasks/agent-doc/plan-closeout-active-prompt-guard.md: fail closeout when a committed response is followed by a newer unresolved prompt-bearing exchange line or when the matching pending item has a malformed checklist prefix.
<!-- /agent:pending-done -->



<!--
I see the following in the binding codex session prompt. Why is it there? Why was it not Entered? spec-test-build-install-commit-push
```
› echo '[agent-doc] startup-miss: routed trigger accepted but no document cycle started for pending prompt_target: do
  #ppct. spec-test-build-install-commit-push. Run '\''agent-doc start tasks/agent-doc/agent-doc-bugs2.md'\'' to retry.'
```
-->
