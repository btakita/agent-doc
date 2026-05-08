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

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7a9b103bd5b68223dac05a4c449ac03899aba53d6022e82100578b5b0-20260508-023542.md`*

Active backlog:
- [ ] [#sqliterepros] [recommended] Before prototyping .agent-doc/state.db, capture 2-3 current agent-doc failures where cross-file authority drift or non-atomic state writes would have been fixed by one transactional store rather than by the single-owner actor/supervisor model alone.
- [ ] [#sqliteeval] [recommended] Review plan tasks/agent-doc/plan-sqlite-session-store-concurrency.md against the single-owner actor rollout and decide whether to prototype .agent-doc/state.db as the authoritative session store.
❯ do [#sqliterepros]
### Re: sqliterepros — gpt-5

Captured three current failure shapes where SQLite would have helped more directly than single-owner actor semantics alone:

1. **Cross-file route rebind / pane theft.** The `#jbpdrop` repro let `tasks/software/tsift.md` temporarily emit `session_superseded old_pane=%177 new_pane=%169` even though `%169` was the authoritative `tasks/agent-doc/agent-doc-bugs2.md` pane. Relevant state lived across `.agent-doc/session-actors.json`, `.agent-doc/sessions.json`, tmux-router state, and session logs. A SQLite transaction could have checked the requested document owner, checked whether the candidate pane was unbound or same-document, inserted one transition, and emitted projections only after commit. The actor model fixes ownership vocabulary, but without a transactional store the transient projection write can still become observable before validation finishes. See [VERSIONS.md](/home/brian/work/btakita/agent-loop/src/agent-doc/VERSIONS.md:161) and [SPEC.md](/home/brian/work/btakita/agent-loop/src/agent-doc/SPEC.md:47).

2. **Mixed-root sync / duplicate pane aliasing.** Recent sync bugs allowed one pane to satisfy multiple visible documents, including root/child workspace confusion between `agent-doc-bugs2.md`, `claudescore-3.md`, and child-root registries. Relevant state lived in more than one nearest `.agent-doc/sessions.json`, the synthetic tmux-router registry, live tmux geometry, and process/session-log provenance. SQLite would make this a single indexed query/transaction over canonical documents, roots, panes, generations, and leases, with a uniqueness invariant such as one live pane claim per generation/window scope before projections are emitted. A per-document actor alone does not prove cross-document uniqueness unless a controller/store can see the whole project claim set. See [VERSIONS.md](/home/brian/work/btakita/agent-loop/src/agent-doc/VERSIONS.md:262), [VERSIONS.md](/home/brian/work/btakita/agent-loop/src/agent-doc/VERSIONS.md:276), and [VERSIONS.md](/home/brian/work/btakita/agent-loop/src/agent-doc/VERSIONS.md:284).

3. **Non-monotonic closeout cycle state after repair/replay.** The `#stphk` shape had a committed cycle later downgraded to `response_captured` or `write_applied` by repair bookkeeping, while the capture ledger and session log already proved `commit_success`. Relevant state was split across `.agent-doc/state/cycles/*.json`, captures, snapshots, the session log, and git `HEAD`. SQLite would let closeout advance as monotonic state in one transaction: `preflight_started -> response_captured -> write_applied -> committed`, rejecting stale repair updates with a compare-and-swap instead of relying on scattered JSON guards. The session actor owns the live pane/session generation, but it does not by itself serialize document closeout phase transitions. See [VERSIONS.md](/home/brian/work/btakita/agent-loop/src/agent-doc/VERSIONS.md:266) and [SPEC.md](/home/brian/work/btakita/agent-loop/src/agent-doc/SPEC.md:24).

Net: the evidence supports continuing `#sqliteeval`, but the stronger target is the Project Controller shape: one live controller serializes decisions, while SQLite persists actor state, leases, dispatch attempts, and closeout transitions transactionally. That matches the existing plan boundary in [plan-project-controller-session-actor-mvp.md](/home/brian/work/btakita/agent-loop/tasks/agent-doc/plan-project-controller-session-actor-mvp.md:60) and the SQLite evaluation plan's Phase 1 criteria in [plan-sqlite-session-store-concurrency.md](/home/brian/work/btakita/agent-loop/tasks/agent-doc/plan-sqlite-session-store-concurrency.md:150).
❯ do [#sqliteeval]
### Re: sqliteeval — gpt-5

Reviewed [plan-sqlite-session-store-concurrency.md](/home/brian/work/btakita/agent-loop/tasks/agent-doc/plan-sqlite-session-store-concurrency.md) against [plan-project-controller-session-actor-mvp.md](/home/brian/work/btakita/agent-loop/tasks/agent-doc/plan-project-controller-session-actor-mvp.md) and the current single-owner actor rollout.

Decision: prototype one project-root `.agent-doc/state.db`, but only as the durable state and journal behind the Project Controller. Do not build a direct path where all Harness Supervisors coordinate with each other through SQLite. That would make individual writes atomic, but it would still leave lease interpretation, stale-owner cleanup, prompt-state decisions, conflict resolution, and repair ordering distributed across multiple live processes.

The right split is: Project Controller serializes project-wide decisions; Session Actor owns each document generation and transition contract; SQLite persists the actor state, generation CAS, transition history, uniqueness checks, dispatch attempts, and projection drift diagnostics; `sessions.json`, `session-actors.json`, tmux-router state, and text logs remain projections or diagnostics during migration.

I recorded that in the SQLite plan by changing its status to `Accepted / scoped behind Project Controller` and adding a `Decision` section. Verification for this doc-only change: `git diff --check -- tasks/agent-doc/plan-sqlite-session-store-concurrency.md` passed.
❯ What are the #next-steps for Project Controller -> Session Actor -> Harness Supervisor -> Harness Child?
### Re: next steps — gpt-5

The next steps should follow the controller plan phases in order, because each layer depends on the one above it being real enough to own authority.

1. `#pcctrl`: build the Project Controller shell first: singleton IPC endpoint, launch lock, lazy connect-or-launch helper, `agent-doc controller status`, and persisted controller epoch/launch mode. This should not change route/start behavior yet.
2. `#pcstore`: put the actor store behind the controller: SQLite-first authoritative writes, `session-actors.json` / `sessions.json` projections, and drift diagnostics.
3. `#pcsuper`: make Harness Supervisors register with the controller and report lifecycle facts into actor transitions instead of mutating ownership-adjacent state directly.
4. `#pcroutes`: move `start`, `route`, and `sync` onto controller authority, leaving tmux-router as layout reconciliation only.
5. `#pcops`: wire operator/editor commands through controller clients and keep destructive repair behind explicit `doctor --repair`-style surfaces.

I added those as backlog items in that priority order. Top backlog item: `#pcctrl`.
❯ Please fix
❯ > ⚠ `[features].codex_hooks` is deprecated. Use `[features].hooks` instead.
### Re: deprecated Codex hook config — gpt-5

Fixed the deprecated Codex feature flag.

What changed:
- Updated `.codex/config.toml` from `codex_hooks = true` to `hooks = true`, which addresses the warning directly.
- Updated `src/agent-doc/src/skill.rs` so future `agent-doc skill install` runs write `features.hooks = true` and remove any stale `features.codex_hooks` key from existing configs.
- Updated the agent-doc tests and documentation/runbooks/spec text to reference `features.hooks = true`.

Verification:
- `cargo test` in `src/agent-doc`: passed.
- `cargo fmt --check` in `src/agent-doc`: passed.
- Searched the agent-doc/Codex instruction surfaces for stale `features.codex_hooks` / `codex_hooks = true` references: none remain.
<!-- agent:boundary:ffb35bd7 -->
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

- [ ] [#pcctrl] Implement Phase A from tasks/agent-doc/plan-project-controller-session-actor-mvp.md: add the project-local controller shell, singleton socket/launch lock, lazy connect-or-launch helper, controller status command, and bootstrap epoch/launch-mode persistence without changing route/start behavior yet.
- [ ] [#pcstore] Implement Phase B from tasks/agent-doc/plan-project-controller-session-actor-mvp.md: move actor-record load/store behind controller commands, write SQLite first, emit session-actors.json and sessions.json as projections, and add projection drift diagnostics.
- [ ] [#pcsuper] Implement Phase C from tasks/agent-doc/plan-project-controller-session-actor-mvp.md: make Harness Supervisor startup register with the controller and report ready/busy/waiting-input/blocked/closed lifecycle facts through controller-owned actor transitions.
- [ ] [#pcroutes] Implement Phase D from tasks/agent-doc/plan-project-controller-session-actor-mvp.md: convert start, route, and sync to read/dispatch through controller authority and keep tmux-router as layout reconciliation only.
- [ ] [#pcops] Implement Phase E from tasks/agent-doc/plan-project-controller-session-actor-mvp.md: wire status/history/attach/restart/clear/doctor through controller clients, keep destructive recovery behind doctor --repair, and expose stage-specific editor diagnostics.
<!-- /agent:backlog -->

<!-- agent:icebox -->
<!-- /agent:icebox -->

## Completed / Reaped

<!-- agent:pending-done -->
- 2026-05-07 [#backlogreap] [recommended] Make completed/reaped backlog items visible in the session document or closeout response so items like #commenttail, #tmuxcoord, and #coordspike do not look silently removed after --pending-done.
- 2026-05-07 [#sqliterepros] [recommended] Before prototyping .agent-doc/state.db, capture 2-3 current agent-doc failures where cross-file authority drift or non-atomic state writes would have been fixed by one transactional store rather than by the single-owner actor/supervisor model alone.
- 2026-05-07 [#sqliteeval] [recommended] Review plan tasks/agent-doc/plan-sqlite-session-store-concurrency.md against the single-owner actor rollout and decide whether to prototype .agent-doc/state.db as the authoritative session store.
<!-- /agent:pending-done -->



<!--
I see the following in the binding codex session prompt. Why is it there? Why was it not Entered? spec-test-build-install-commit-push
```
› echo '[agent-doc] startup-miss: routed trigger accepted but no document cycle started for pending prompt_target: do
  #ppct. spec-test-build-install-commit-push. Run '\''agent-doc start tasks/agent-doc/agent-doc-bugs2.md'\'' to retry.'
```
-->
