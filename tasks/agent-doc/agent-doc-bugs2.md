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

Planned editor autosync, sync latency, and tmux test-budget follow-ups.
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7a9b103bd5b68223dac05a4c449ac03899aba53d6022e82100578b5b0-20260508-182851.md`*
❯ The autosync when switching the editor to another agent document + `Sync Tmux Layout` does not change the focus or the visible tmux panes in the agent-doc tmux window.
❯ spec-test-build-install-commit-push
### Re: protected tmux sync — gpt-5

Fixed the no-op sync path.

Root cause: the protected-layout guard preserved the entire visible `agent-doc` tmux window whenever a requested hidden document coincided with a visible pane that owned an open closeout cycle. That was too broad: when another unprotected visible pane can be safely displaced, sync should bring the requested document into view and focus it.

Changed:
- `src/agent-doc/src/sync.rs` now counts detachable unprotected unwanted visible panes and only preserve-warns when missing requested panes outnumber them.
- Added tmux-backed regressions for both passive editor autosync and manual sync.
- Updated `specs/07-session-tmux-commands.md` and `VERSIONS.md` for the new contract.

Verification:
- `cargo fmt`
- `cargo test sync_replaces_detachable_visible_pane_while_protected_pane_remains -- --nocapture`
- `cargo test`
- `cargo build`
- `make check` in `src/agent-doc`
- `make install`
- `agent-doc --version`

Note: root `make check` still fails before running project checks because the root `Makefile` tries `cd instruction-files`, but this checkout has that crate at `src/instruction-files`. The relevant `src/agent-doc` `make check` suite passes.

Committed and pushed:
- `ee05a20` in `src/agent-doc`: `fix: unblock protected layout sync replacement`
- `174cc0052` in `agent-loop`: `agent-doc: update protected sync replacement`
- Changing editor files does not sync the tmux pane. `Sync Tmux Layout` does change the tmux pane.
- `Sync Tmux Layout` has a few second lag...which was introduced when we moved to the Controller architecture.
- With the new SimWorld + Tiger Beetle style of testing, do we still need so many slow tmux integration tests for edge cases?

❯ Please plan #next-steps for the 3 items.
### Re: next steps — gpt-5

Created `tasks/agent-doc/plan-editor-autosync-latency-test-budget.md` and committed it as `35e25f751`.

Priority order:

1. `#autosync` - fix editor selection autosync correctness first. Manual `Sync Tmux Layout` working means the immediate gap is likely in the automatic editor path, its focus-vs-sync planning, dedup/generation guard, or how passive preserve/reselect output is treated.
2. `#synclag` - make the controller-era sync lag accountable second. The plan calls for real slow-run traces, phase-level `sync_latency`/controller buckets, and a warm-controller budget so a multi-second sync names its exact cause.
3. `#tmuxbudget` - reduce slow tmux integration coverage third. We should keep real tmux tests for pane/window/shell/process mechanics, but pure ownership schedules, stale generations, projection drift, missing panes, and dispatch-proof combinations should move into deterministic SimWorld traces once the behavior contract is stable.

Top of backlog: `#autosync`.
<!-- agent:boundary:5ffe72b6 -->
<!-- /agent:exchange -->
###

<!--
Running one turn on a document should not prevent another document from syncing in any step. The syncs should be "snappy".
```
Sync deferred: another visible agent-doc pane is mid-closeout, so the current tmux layout was preserved. Try again after that closeout finishes. Blocked pane(s): %268 preflight_started /home/brian/work/btakita/agent-loop/tasks/agent-doc/agent-doc-bugs2.md
```
spec-test-build-install-commit-push
---
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

- [ ] [#autosync] Implement Phase 1 from tasks/agent-doc/plan-editor-autosync-latency-test-budget.md: fix editor selection autosync so changing focused markdown files applies the same tmux pane/focus result as manual Sync Tmux Layout, with editor and CLI regressions.
- [ ] [#synclag] Implement Phase 2 from tasks/agent-doc/plan-editor-autosync-latency-test-budget.md: capture slow manual sync runs, split sync/controller latency buckets, keep warm-controller safe-passive sync under budget, and surface the exact over-budget phase.
- [ ] [#tmuxbudget] Implement Phase 3 from tasks/agent-doc/plan-editor-autosync-latency-test-budget.md: inventory slow tmux-backed edge tests, keep only real tmux smoke coverage, and migrate pure ownership/controller schedules into deterministic SimWorld traces with runtime-budget tracking.
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
- 2026-05-08 [#tigersim] [recommended] Implement Phase 1-2 from tasks/agent-doc/plan-tigerbeetle-style-simulation-testing.md: inventory pure closeout seams, build the first SimWorld closeout simulator, and promote fixed seeds for recent closeout/pending/snapshot bugs.
- 2026-05-08 [#tigerfault] Implement Phase 3 from /home/brian/work/btakita/agent-loop/tasks/agent-doc/plan-tigerbeetle-style-simulation-testing.md: add deterministic crash and fault injection points around snapshot save, fallback patch write, IPC delivery, template merge, working-tree write, index update, git commit, post-commit boundary reposition, and session-check.
- 2026-05-08 [#harnessprompt] Implement tasks/agent-doc/plan-harness-prompt-no-changes-closeout.md: make harness-native agent-doc turns fail closed when preflight reports no markdown changes but the chat prompt contains actionable text that was never persisted to the exchange.
- 2026-05-08 [#tigerroute] Implement Phase 4 from /home/brian/work/btakita/agent-loop/tasks/agent-doc/plan-tigerbeetle-style-simulation-testing.md: extend SimWorld with route/controller schedules, session actor generations, supervisor lifecycle facts, dispatch acceptance/proof, stale/missing pane observations, and projection-drift invariants.
- 2026-05-08 [#tigerci] Implement Phase 5 from /home/brian/work/btakita/agent-loop/tasks/agent-doc/plan-tigerbeetle-style-simulation-testing.md: wire fast fixed-seed simulator coverage into cargo test, add a wider deterministic make check budget if runtime stays acceptable, document seed-range replay/promotion, and track generated-coverage runtime budgets.
<!-- /agent:pending-done -->



<!--
I see the following in the binding codex session prompt. Why is it there? Why was it not Entered? spec-test-build-install-commit-push
```
› echo '[agent-doc] startup-miss: routed trigger accepted but no document cycle started for pending prompt_target: do
  #ppct. spec-test-build-install-commit-push. Run '\''agent-doc start tasks/agent-doc/agent-doc-bugs2.md'\'' to retry.'
```
-->
