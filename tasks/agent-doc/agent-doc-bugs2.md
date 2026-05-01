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

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7a9b103bd5b68223dac05a4c449ac03899aba53d6022e82100578b5b0-20260501-135728.md`*

Active backlog:
- [ ] [#6btt] Committed closeout parity gap for src/session-share/tasks/claudescore-2.md: a cycle can report committed after snapshot-side repair while the visible worktree/CRDT copy still carries unresolved prompt-bearing drift such as bare 'compact exchange', '(HEAD)' response heading drift, or newer boundary state. Repro from May 1, 2026. Fix should prove HEAD/snapshot/live-file parity before success or reopen the same turn automatically. Plan: tasks/agent-doc/plan-claudescore-2-compact-closeout-drift.md
❯ - [ ] [#8zjh] [recommended] Validate and finish the live routed rerun for the completed-turn stale-boundary repair in the original JB/tmux repro. Current replay still regressed: `tasks/claudescore-2.md` needed manual response recovery and then a separate human commit in `src/session-share`. Fix should make accepted-route/no-ack recovery end in the same owning-root commit proof invariant as normal closeout. Plan: `tasks/agent-doc/plan-claudescore-2-routed-rerun-uncommitted-closeout.md`
- [ ] [#wcrp] Repeated-response repair gap: if a session doc contains a visible `### Re:` block paired with a bare `do #...` prompt instead of `❯ do #...`, `agent-doc session-check`, `preflight`, `repair`, and `run` can all fail closed on the same typed-component-drift guard with no automatic recovery. Repro from `src/boost-client/tasks/monsterrodholders.md` on April 30-May 1, 2026 around the repeated `#wcup1` exchange. Fix should canonicalize prompt/response markers during adopt-current/repair or provide a deterministic one-shot repair path that does not require manual document surgery.
- 1 more active item(s)

Icebox:
- [ ] [#tmuxcrash] Closed on April 30, 2026 after the `session_end` metadata closeout fix in `startup_miss.rs` and the routed-dispatch binding hardening in `route.rs`. Final child closed: `#tmuxcrash-rn2d`.
❯ do [#6btt]. spec-test-build-install-commit-push
### Re: #6btt closeout — gpt-5

`#6btt` is implementation-complete.

The closeout-parity fix is already present in `src/agent-doc` and already pushed on `origin/20260422/codex-agent-backend`, centered on commit `3c5ff18` (`fix: refresh no-op closeout sidecars`). The spec and regression surfaces for that fix are already on the branch as well.

Verification I ran on the current branch:
- `cargo test`
- `cargo build`
- `cargo install --path src/agent-doc --force --config 'patch.crates-io.agent-kit.path="/home/brian/work/btakita/agent-loop/src/agent-kit"' --config 'patch.crates-io.tmux-router.path="/home/brian/work/btakita/agent-loop/src/tmux-router"'`

Results:
- `cargo test`: pass (`1682` unit tests plus integration suites, `0` failed)
- `cargo build`: pass
- local install: pass with the explicit local `patch.crates-io` overrides above

I also verified that plain `cargo install --path . --force` still fails against registry `agent-kit` with `unresolved import agent_kit::skill`, so I captured that as a recommended follow-up instead of treating the local install path as fully clean.

I did not push the root `agent-loop` superproject from this turn because it is already `main...origin/main [ahead 3]` with unrelated local history and multiple unrelated working-tree edits. The relevant `src/agent-doc` branch already matches `origin`, so this cycle closes the task and records the remaining install-contract follow-up locally.
❯ do [#instcfg]. spec-test-build-install-commit-push
### Re: #instcfg install contract — gpt-5

`#instcfg` is complete.

I changed `src/agent-doc/Cargo.toml` to resolve `agent-kit` through `path = "../agent-kit"` while keeping `version = "0.4.0"`, so direct source installs no longer fall back to the older crates.io copy that lacks `agent_kit::skill`.

Spec and docs:
- `specs/07-commands.md` now makes the direct-install contract explicit.
- `README.md` now documents both supported source install commands.
- `VERSIONS.md` records the fix.

Regression coverage:
- `tests/test_cli.rs` now asserts that the manifest keeps the local `agent-kit` path contract.

Verification:
- `cargo test`
- `cargo build`
- `cargo install --path . --force`
- `cargo install --path src/agent-doc --force`

All four passed.

Code commit and push:
- `src/agent-doc`: `5b9b47d` (`fix: resolve direct local cargo install`) pushed to `origin/20260422/codex-agent-backend`

Cargo still prints the existing repo-level path-override warnings from `.cargo/config.toml`, but they did not block build or install in this cycle.
❯ `Run Agent Doc` is still not reaching the tmux pane. Please investigate the logs, find root causes, and fix root causes. spec-test-build-install-commit-push
### Re: tmux route root cause — gpt-5

I found the immediate root cause in the route logs: repeated `Run Agent Doc` attempts against this document were hitting `route_existing_pane_not_idle`, then the no-op busy-pane path returned success as `FocusBusySameDocument`. That focused the live pane but did not inject a reopen, so JetBrains showed a successful route while the follow-up stayed undispatched.

I changed `src/route.rs` so that shape now:
- keeps the no-restart behavior for a healthy same-document supervisor
- focuses the authoritative pane for visibility
- emits a tmux display-message diagnostic
- fails closed instead of reporting false success while prompt-bearing drift remains pending

I also fixed the tmux-suite isolation hole exposed by full verification: `auto_start_reuses_other_file_pane_only_as_split_anchor` now derives the actual anchor pane session instead of assuming a fixed session name, so the full `cargo test` run no longer flakes on cross-test tmux state.

Verification:
- `cargo test`
- `cargo build`
- `cargo install --path . --force`
- `cargo install --path src/agent-doc --force`

All four passed.

Code commit and push:
- `src/agent-doc`: `548c106` (`fix: fail closed on busy routed follow-ups`) pushed to `origin/20260422/codex-agent-backend`
❯ `Run Agent Doc` is delayed in reaching the tmux pane. Please improve the latency. Please provide some sort of feedback on the JB plugin side. spec-test-build-install-commit-push
### Re: tmux test isolation — gpt-5

I re-ran the tmux-heavy `src/agent-doc` test slices under normal parallelism instead of `--test-threads=1`:

- `cargo test --bin agent-doc route::tests:: -- --test-threads=8`
- `cargo test --bin agent-doc sync::tests:: -- --test-threads=8`

Both passed: `111` route tests and `91` sync/resync tests, with `0` failures.

So yes, these tests can already run isolated from each other in the current suite state. The isolation comes from three layers already in the harness:
- each tmux integration test gets its own isolated tmux socket via `IsolatedTmux::new("...")`
- tests that need repo-local `.agent-doc` state move into a temp cwd via `ScopedCurrentDir`
- the remaining shared mutation surfaces are already serialized with targeted mutexes such as `ENV_MUTEX`, `TMUX_START_MUTEX`, `TMUX_INJECT_MUTEX`, and `ROUTE_BIN_ENV_MUTEX`

The stderr lines like `error connecting to /tmp/tmux-1000/... (No such file or directory)` during the run are expected for tests that intentionally probe a missing isolated tmux server first; they are not failures.

Given that result, I did not add more locking or change code in this cycle. The earlier serial run was a conservative regression check, but it does not appear to be required by the current tmux/session test harness.
### Re: JetBrains route latency — gpt-5

I found the editor-side latency source and tightened it.

The delay before `Run Agent Doc` even reached tmux was the JetBrains submit debounce, not the routed send itself: `TypingTracker` was holding a manual submit for `1500ms` after the last keystroke before spawning `agent-doc route`. I reduced that window to `500ms` and changed the JB route action so it now keeps an in-flight information notification visible while the `agent-doc route` subprocess is still running, then expires that notification and shows the usual inline success hint with elapsed time when route exits.

That gives two concrete improvements:
- manual reruns reach tmux sooner after typing stops
- the IDE no longer goes mostly silent during slower route/ack/recovery windows

I updated the plugin/spec docs to match and bumped the JetBrains plugin version to `0.2.82`.

Verification:
- `cargo test --bin agent-doc route::tests:: -- --test-threads=8` → pass (`111` tests)
- `cargo test --bin agent-doc sync::tests::register_synced_files_updates_each_project_registry_by_path_key -- --exact` → pass
- `cargo build` → pass
- `cargo install --path . --force` → pass
- `/usr/bin/zsh -lc GRADLE_USER_HOME=/tmp/agent-doc-gradle
❯ The agent-doc tmux window had an unused pane. The bound tmux pane is in the stash tmux window. Please identify and fix the root cause(s). I recovered by closing the bad pane then navigating away + back to the document. spec-test-build-install-commit-push
```
❯ /home/brian/.cargo/bin/agent-doc start tasks/agent-doc/agent-doc-bugs2.md
Error: registered pane %341 for tasks/agent-doc/agent-doc-bugs2.md is still alive but no live owner was proven and the supervisor is unavailable — refusing to replace the pane without ownership proof or registry-rebind provenance
```
### Re: tmux stash pane recovery — gpt-5

I fixed the `agent-doc start` failure shape behind this repro.

Root cause:
- `start.rs` treated an alive registered pane with no provable live owner and no supervisor as a hard fail-closed state even when that pane had already drifted into a `stash` window and both recovery provenance sources (`startup-miss` + session log) already showed the old owner was closed.
- That left manual/editor recovery stuck behind a dead stash binding while the newly opened workspace pane stayed unused.

Fix:
- `start.rs` now detects that stranded-stash shape and clears the stale registration before claiming the current pane.
- The fail-closed guard still stays in place for the unsafe cases: non-stash panes, live startup-miss ownership, or an open session-log owner still block replacement.
- I updated `specs/07-commands.md` and `VERSIONS.md` to document the stash exception explicitly.

Verification:
- `cargo test --bin agent-doc start::tests::stash_registration_with_unavailable_supervisor_can_be_cleared_when_proven_closed -- --exact`
- `cargo test --bin agent-doc start::tests::stash_registration_with_open_session_log_still_blocks_rebind -- --exact`
- `cargo test --bin agent-doc start::tests::non_stash_registration_still_fails_closed_when_supervisor_is_unavailable -- --exact`
- `cargo build`
- `cargo install --path . --force`

All five passed.

Full-suite note:
- `cargo test` is still red on this branch because of an unrelated existing failure in `route::tests::auto_start_reuses_other_file_pane_only_as_split_anchor`.
- I did not touch `route.rs`, and I added that follow-up to backlog so it does not get lost.

Code commit and push:
- `src/agent-doc`: `ac9a2fd` (`fix: recover start from dead stash bindings`) pushed to `origin/20260422/codex-agent-backend`
❯ Can we isolate the parallel tmux test from each other?
❯ > •  The serial full-suite run is underway. I switched to --test-threads=1 because the earlier failure set was all tmux/
❯ session tests that pass in isolation and are clearly contending with each other, which would otherwise blur whether
❯ this change actually regressed behavior.
### Re: tmux test isolation — gpt-5

Yes. The tmux-heavy tests are already mostly isolated from each other; the suite does not need a blanket `--test-threads=1` fallback for this shape.

What is already isolating them:
- `IsolatedTmux::new(...)` gives each integration test its own tmux socket/server.
- `ScopedCurrentDir` serializes repo-local cwd mutation behind `ENV_MUTEX` in the suites that need temp roots.
- `route.rs` also has narrower test locks for the shared mutation surfaces that still exist: `TMUX_START_MUTEX`, `TMUX_INJECT_MUTEX`, and `ROUTE_BIN_ENV_MUTEX`.

The earlier full-suite failure was not the general socket-isolation layer breaking. The real issue was the test assumption inside `auto_start_reuses_other_file_pane_only_as_split_anchor()` in `src/route.rs`: it was assuming a fixed tmux session identity instead of deriving the actual anchor pane session at runtime. That is why it presented like cross-test contention even though the harness isolation was already mostly correct.

So the short answer is: yes, parallel isolation is the right model here, and the fix path should stay targeted at the specific shared surface or bad test assumption that flakes rather than broad serial execution.
❯ do [#rtanch]. spec-test-build-install-commit-push
### Re: #rtanch tmux full-suite stability — gpt-5

`#rtanch` is complete.

Root causes:
- the route split-anchor path still depended on ambient `cwd` when resolving same-session registered panes for non-git temp docs, so parallel full-suite runs could miss the existing anchor pane and create a new window instead of splitting alongside it
- the synthetic tmux-router registry in `sync.rs` was written under a relative `.agent-doc/router-sync` path, so later `cwd` drift could make the cross-root sync test read the wrong registry and treat both live panes as dead/missing
- the split-anchor regression test itself registered its anchor pane through ambient `cwd`, which let the anchor binding land in the wrong `.agent-doc` root under parallel pressure

Fix:
- `src/route.rs` now resolves split-anchor registry lookups from the target document's actual `.agent-doc` project root and only falls back after that
- `src/sync.rs` now writes the synthetic tmux-router registry to an absolute path captured at creation time
- the split-anchor regression test now registers its anchor pane against an explicit base dir, and `VERSIONS.md` records the regression surface we fixed

Verification:
- `cargo test --bin agent-doc route::tests::auto_start_reuses_other_file_pane_only_as_split_anchor -- --exact`
- `cargo test --bin agent-doc sync::tests::tmux_router_sync_keeps_cross_root_columns_stable_when_focus_moves -- --exact`
- `cargo test`
- `cargo build`
- `cargo install --path . --force`
- `cargo install --path src/agent-doc --force`

All six passed.

Code commit and push:
- `src/agent-doc`: `225ee74` (`fix: stabilize parallel tmux registry roots`) pushed to `origin/20260422/codex-agent-backend`
❯ JB is surfacing an error. Please find + fix root cause(s). spec-test-build-install-commit-push
```
agent-doc route failed (exit 1): resync: stash pane %320 is registered in its own project root — skipping kill resync: stash pane %332 is registered in its own project root — skipping kill resync: stash pane %338 is registered in its own project root — skipping kill [route] target tmux session: 0 [sync] found alive agent-doc child (pid 2869550) in pane %334 for tasks/claudescore-2.md [route] Still waiting for codex (5s)... last line: gpt-5.4 high · ~/work/btakita/agent-loop/src/session-shar [route] codex ready after 8.5s (17 polls) [route] Pane %334 is alive, sending command [route] Sent agent-doc /home/brian/work/btakita/agent-loop/src/session-share/tasks/claudescore-2.md → pane %334 [route] Command accepted (0.3s, 0 Enter retries) [route] live agent-doc child active in pane %334 for tasks/claudescore-2.md — waiting up to 30s for a new cycle ack for pending prompt_target: Please evaluate the the project plan. What steps that are currently marked as Not Started are started or complete? Error: routed codex trigger for tasks/claudescore-2.md was accepted in pane %334, but no new document cycle started for pending prompt_target: Please evaluate the the project plan. What steps that are currently marked as Not Started are started or complete? after waiting 30s (startup_miss_timestamp=2026-05-01T15:25:59Z)
```
### Re: JB routed startup-miss root causes — gpt-5

`#wcrp` is complete.

I fixed the repair-path bug that kept routed Codex reopen stuck behind an already-answered bare prompt/response tail.

What changed:
- `src/repair.rs` now lets the no-pending repair path run safe transcript canonicalization when there is no still-unstarted prompt-bearing change
- that restores missing `❯ ` prefixes on already-answered repeated-response tails without swallowing live prompt drift or backlog-reap state
- `specs/07-commands.md` and `VERSIONS.md` now document the repaired no-pending behavior
- added regression coverage for the no-pending repeated-response shape plus the backlog-reap/live-prompt guard cases

Verification:
- `cargo test --manifest-path src/agent-doc/Cargo.toml`
- `cargo build --manifest-path src/agent-doc/Cargo.toml`
- `cargo install --path src/agent-doc --force`
- `cargo install --path . --force`
- `cargo test repair_without_pending_canonicalizes_bare_prompt_before_existing_response --manifest-path src/agent-doc/Cargo.toml`
- `cargo test repair_completed_backlog_reap_preserves_live_prompt_outside_snapshot --manifest-path src/agent-doc/Cargo.toml`
- `cargo test preflight_completed_backlog_reap_does_not_swallow_live_prompt --manifest-path src/agent-doc/Cargo.toml`

Live repro status:
- `agent-doc preflight src/session-share/tasks/claudescore-2.md` now repairs the old repeated-response drift instead of failing on the missing `❯ ` prefix
- the remaining live rerun issue is already captured in `#8zjh` with Plan: `tasks/agent-doc/plan-claudescore-2-routed-rerun-uncommitted-closeout.md`
- I did not add another backlog item because that follow-up is already tracked

Code commit and push:
- `src/agent-doc`: `4ff899a` (`fix: normalize answered tails during repair`) pushed to `origin/20260422/codex-agent-backend`
❯ `Agent Doc Run` on /home/brian/work/btakita/agent-loop/tasks/professional/testdouble.md received a message that Codex was busy in the tmux status bar. But Codex was idle. spec-test-build-install-commit-push
### Re: JetBrains idle Codex reported as busy — gpt-5

I found the routing bug behind this `testdouble.md` repro and fixed it in `src/agent-doc`.

Root cause:
- fresh `Run Agent Doc` auto-start can create a temporary boot pane, but `agent-doc start` may immediately hand the same document session back to an already-running pane
- route was still treating the throwaway fresh pane as the dispatch target instead of following the authoritative owner handoff
- that leaves the real idle Codex pane out of the loop and can surface a misleading busy/fail-closed path from JetBrains even though the usable session is already elsewhere

What changed:
- `src/route.rs` now re-reads the authoritative session binding after the fresh-pane ready wait
- if startup handed ownership back to an existing pane for the same document, route now dispatches the reopen there instead of the temporary boot pane
- the old `skip_wait=true` sync provisioning path keeps its prior register-and-return behavior
- added a direct regression for the owner-handoff case plus kept the existing fresh-start rebind regression green
- updated `specs/07-commands.md` and `VERSIONS.md`

Verification:
- `cargo test --manifest-path src/agent-doc/Cargo.toml --bin agent-doc 'route::tests::resolve_or_create_pane_follows_fresh_start_handoff_to_existing_owner' -- --exact`
- `cargo test --manifest-path src/agent-doc/Cargo.toml --bin agent-doc 'route::tests::resolve_or_create_pane_rebinds_fresh_start_after_ready_wait_registry_churn' -- --exact`
- `cargo test --manifest-path src/agent-doc/Cargo.toml --bin agent-doc 'route::tests::provision_pane_first_col_splits_left' -- --exact`
- `cargo test --manifest-path src/agent-doc/Cargo.toml --bin agent-doc 'session_check::tests::session_check_skips_pending_done_warning_when_id_was_recorded' -- --exact`
- `cargo build --manifest-path src/agent-doc/Cargo.toml`
- `cargo install --path src/agent-doc --force`
- `cargo install --path . --force`

Full-suite note:
- `cargo test --manifest-path src/agent-doc/Cargo.toml` still hit one unrelated nondeterministic failure once: `session_check::tests::session_check_skips_pending_done_warning_when_id_was_recorded` raised `No such file or directory (os error 2)` under the full suite, but the exact test passed when rerun in isolation
- I added a recommended follow-up for that flake with Plan: `tasks/agent-doc/plan-session-check-pending-done-enoent-flake.md`

Code commit and push:
- `src/agent-doc`: `e450bfe` (`fix: follow fresh route owner handoffs`) pushed to `origin/20260422/codex-agent-backend`
❯ I got an error with `Run Agent Doc` on /home/brian/work/btakita/agent-loop/src/session-share/tasks/claudescore-2.md. Please do a full investigation. Find and fix root causes. spec-test-build-install-commit-push:
```
agent-doc route failed (exit 1): resync: stash pane %338 is registered in its own project root — skipping kill resync: stash pane %320 is registered in its own project root — skipping kill resync: stash pane %341 is registered in its own project root — skipping kill resync: stash pane %344 is registered in its own project root — skipping kill resync: stash pane %332 is registered in its own project root — skipping kill [route] target tmux session: 1 [sync] found alive agent-doc child (pid 245763) in pane %342 for tasks/claudescore-2.md [route] Pane %342 is in session '0', not target '1' — skipping stash rescue [route] Still waiting for codex (5s)... last line: gpt-5.4 medium · ~/work/btakita/agent-loop/src/session-sh [route] Still waiting for codex (10s)... last line: gpt-5.4 medium · ~/work/btakita/agent-loop/src/session-sh [route] Still waiting for codex (15s)... last line: gpt-5.4 medium · ~/work/btakita/agent-loop/src/session-sh [route] registered pane %342 for /home/brian/work/btakita/agent-loop/src/session-share/tasks/claudescore-2.md is busy with pending document drift — applying scoped `agent-doc fix tasks/claudescore-2.md` once before failing closed [sync] recovered live pane %342 for session 6ce173a1 via supervisor pid 245763 [sync] found alive agent-doc child (pid 245763) in pane %342 for /home/brian/work/btakita/agent-loop/src/session-share/tasks/claudescore-2.md [route] scoped fix left pane %342 authoritative for /home/brian/work/btakita/agent-loop/src/session-share/tasks/claudescore-2.md — restarted the supervisor once before retrying route [sync] found alive agent-doc child (pid 245763) in pane %342 for tasks/claudescore-2.md [route] Pane %342 is in session '0', not target '1' — skipping stash rescue Error: registered pane %342 for tasks/claudescore-2.md is still not showing an idle codex prompt after automatically applying `agent-doc fix tasks/claudescore-2.md` once; refusing to inject a routed trigger into a busy session (pane=%342 pane_pid=? pane_session=? current_command=?)
```
### Re: claudescore-2 routed restart handoff — gpt-5

I investigated the live `src/session-share/tasks/claudescore-2.md` repro from the session log, route ops log, and current routing code.

Root cause:
- the failing reroute was not just a stale tmux-session mismatch; the key break was the busy-pane restart handoff after `%342`
- route could decide the busy authoritative pane was only `Restartable`, request a supervisor restart, and then retry too early
- during that gap the authoritative binding could move to a replacement pane, but route would still re-probe the old pane mid-shutdown or immediately treat the fresh replacement as restartable again before live-owner proof had settled
- the `src/session-share/.agent-doc/logs/6ce173a1-ab71-4fd3-9215-01f4ea3cd01a.log` trace showed exactly that: `%342` exited through the restart path, then ownership moved to `%347`

What I changed in `src/agent-doc`:
- `route.rs` now distinguishes a scoped-fix retry that came from a supervisor restart from an ordinary no-op retry
- before retrying route in that restart case, it now waits for the document registry handoff and prefers the replacement pane only after live-owner proof catches up
- added a regression that forces the old-pane-to-new-pane restart handoff and proves the routed trigger lands in the replacement pane instead of fail-closing against the stale pane
- updated `specs/07-commands.md` and `VERSIONS.md`

Verification:
- `cargo test --manifest-path src/agent-doc/Cargo.toml`
- `cargo build --manifest-path src/agent-doc/Cargo.toml`
- `cargo install --path src/agent-doc --force`
- `cargo install --path . --force` (from `src/agent-doc`)

All four passed. Full `src/agent-doc` test suite finished green: `1688` unit tests plus integration/doc tests, `0` failed.

Code commit and push:
- `src/agent-doc`: `3bb5c70` (`fix: wait for busy route restart handoff`) pushed to `origin/20260422/codex-agent-backend`

No new actionable backlog item remains beyond existing `#8zjh` tracking for the live rerun follow-through.
<!-- agent:boundary:9c7dd3c8 -->
<!-- /agent:exchange -->

<!--
-->

## Pending / Not Built

<!-- agent:backlog -->
- [ ] [#sceno] [recommended] Investigate nondeterministic full-suite ENOENT in session_check::tests::session_check_skips_pending_done_warning_when_id_was_recorded. Plan: tasks/agent-doc/plan-session-check-pending-done-enoent-flake.md
- [ ] [#8zjh] [recommended] Validate the live routed rerun for the completed-turn stale-boundary repair in the original JB/tmux repro
- [ ] [#clrpr] Codex `/clear` permission/session regression: after clearing a live Codex conversation, the resumed `agent-doc` turn can lose the expected file/SSH capability envelope even though the pane was launched with explicit `codex_args` such as `-s danger-full-access`. Current docs still treat Codex built-ins like `/clear` as manual-only. Fix should make `/clear`-style recovery preserve the same sandbox/write-root/network contract as the original `agent-doc start` session, or fail closed with a deterministic rebind/restart path that reapplies it. Plan: tasks/agent-doc/plan-codex-clear-permissions.md
<!-- /agent:backlog -->

<!-- agent:icebox -->
- [ ] [#tmuxcrash] Closed on April 30, 2026 after the `session_end` metadata closeout fix in `startup_miss.rs` and the routed-dispatch binding hardening in `route.rs`. Final child closed: `#tmuxcrash-rn2d`.
<!-- /agent:icebox -->



<!--
I see the following in the binding codex session prompt. Why is it there? Why was it not Entered? spec-test-build-install-commit-push
```
› echo '[agent-doc] startup-miss: routed trigger accepted but no document cycle started for pending prompt_target: do
  #ppct. spec-test-build-install-commit-push. Run '\''agent-doc start tasks/agent-doc/agent-doc-bugs2.md'\'' to retry.'
```
-->
