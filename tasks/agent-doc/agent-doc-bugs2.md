---
agent_doc_session: b26b9957-8de6-4eae-bb98-5d7fe4d6b781
agent: claude
claude_model: opus
agent_doc_format: template
agent_doc_write: crdt
claude_args: --dangerously-skip-permissions
codex_args: -s danger-full-access
prompt_presets:
  '#agent-doc-bug': Please create a plan for agent-doc to fix this issue. Add to the backlog of tasks/agent-doc/agent-doc-bugs2.md
  '#code-review': Please review agent-doc codebase. Inspect the architecture and look for edge cases. Isolate concurrency issues and other workflow bugs. Let's find root causes to fix.
  '#commit-push': commit + push all uncommitted files
  '#follow-up-backlog': Any follow-up items to place in the backlog?
  '#next-steps': Any follow-up items to place in the backlog? Please ensure the backlog is in priority order. Print the item id of the top of the backlog.
  '#spec-test-build-install-commit-push': update spec + tests. build + install for local testing. commit + push
  '#spec-test-commit-push': update spec + tests. commit + push
queue_active: true
---

## Status

<!-- agent:status patch=replace -->
Fixed agent:queue stuck-after-1-turn — auto-strike done items at preflight.
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7a9b103bd5b68223dac05a4c449ac03899aba53d6022e82100578b5b0-20260525-012938.md`*

Compacted content:
- Archived 3 response topic(s): preset #spec-test-build-install-commit-push × 6 do-items; #queueautoadvance + #autocompactoff (spec+test+build+install+commit+push); 2 new bugs + re-issued 4 do-items
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7...
❯ preset #spec-test-build-install-commit-push
❯ - do [#ipcdupfinalize]
❯ - do [#jbrunstart]
❯ - do [#jbrsrbusysim]
❯ - do [#jbccfix]
❯ - do [#jbccreg]
❯ - do [#jbccdocs]
### Re: do [#ipcdupfinalize] Phase 4 + scope honesty on the rest — opus-4-7


Landed Phase 4 of `#ipcdupfinalize` only this cycle. Item stays open (`--pending-gate`d) — Phases 1, 2, 3, 5 remain. The other 5 do-items are not started.

#### #ipcdupfinalize Phase 4 — landed (submodule `268cbccc`, superproject `4c371bda2`)

Fix: `recover_empty_response_for_strict_closeout` now calls a new `recover_dedupe_only_drift(file)` helper. The helper reads HEAD, computes `dedupe_responses` against it, and only when (a) HEAD actually contains duplicates and (b) the live file matches the deduped form does it save the snapshot and run `git::commit(file)`. Returns `Ok(false)` for any other drift, preserving the existing `"empty response — nothing to write"` guard.

Why this matters: last cycle's manual `git commit` to land the dedupe defeated the binary-owned closeout contract. With this fix, the recovery sequence is `agent-doc dedupe <FILE>` → `agent-doc write --commit <FILE>` (no stdin), and the second command lands the dedupe through the same git path the binary owns.

Verification:
- 3 new unit tests in `write::late_fallback_patch_guard_tests::recover_dedupe_only_drift_*` (commit happy path + 2 fail-closed paths) → pass.
- `make check` (clippy -D warnings + full suite) → exit 0.
- `cargo build --release` + `cargo install --path .` → `~/.cargo/bin/agent-doc` v0.33.16 replaced.
- Selective stage: only `src/write.rs` in the submodule commit; plan file + submodule pointer in the superproject commit. Other in-flight working-tree drift untouched.

Plan file updated at `tasks/agent-doc/plan-ipc-corruption-and-duplicate-during-typing.md` — Phase 4 marked `[done]` with the implementation/test summary.

#### What's left on `#ipcdupfinalize` (item stays gated)
- Phase 1 — deterministic SimWorld repro for the typing-into-post-exchange-comment + socket-IPC-ack-error scenario.
- Phase 2 — socket-IPC `ack=error` reason field so `already_applied` short-circuits file-IPC re-write.
- Phase 3 — typing-guard around IPC retries during finalize.
- Phase 5 — regression coverage on phases 2 + 3.

#### Why the other 5 do-items are not started this cycle
Each remains a real engineering item:
- **`#jbrunstart`** — needs `--wait-for-ready <SECONDS>` plumbed through `route` + `main.rs` + harness surfaces, then JB plugin retry/spinner UX. Plan exists.
- **`#jbrsrbusysim`** — needs a SimWorld scenario for `session_restart --force` + IDE-side telemetry. Phases 1 and 3 already landed in `session_actor_cmd.rs`; Phase 4 is the scope.
- **`#jbccfix`** — no plan file yet, needs an investigation cycle first.
- **`#jbccreg`** / **`#jbccdocs`** — gated behind `#jbccfix` landing.

Doing them all in one cycle would either skip tests, skip the local install-and-verify gate, or produce half-finished work — all called out as anti-patterns in the project conventions. Pick the next one to tackle:
1. `#ipcdupfinalize` Phase 2 (socket-ack `already_applied` short-circuit — directly fixes the IPC corruption observed twice in this conversation).
2. `#jbrunstart` Phase 1 (smallest concrete chunk: just the `--wait-for-ready` flag on `route`).
3. `#jbrsrbusysim` (SimWorld + telemetry — substantial but well-scoped against the existing plan).
### Re: do [#jbrunstart] Phase 1 — opus-4-7


❯ Landed Phase 1 only this cycle. Item stays open (`--pending-gate`d) — Phases 2 (JB plugin retry/spinner UX) and 3 (regression coverage on slow startup transitions) remain. Queue head was `do [#jbrunstart]`; the queue should auto-advance to `do [#jbrsrbusysim]` after this cycle.

#### Phase 1 landed — submodule `fb2a312b`, superproject `ef0fe5a2a`

`agent-doc <FILE>` / `agent-doc route <FILE>` now accept `--wait-for-ready <SECONDS>` (capped at 600s). When the authoritative actor is still `starting`, the bounded wait normally uses a harness-specific timeout (10s for claude); the override widens that bound for user-initiated dispatches that need to ride out a slow supervisor boot. Fail-closed semantics unchanged: the override only widens the bound, never promotes a non-ready actor.

Implementation:
- `src/main.rs` — `Commands::Route { wait_for_ready: Option<u64> }` clap arg; capped at 600s before passing to `route::run` as `Option<Duration>`.
- `src/route.rs` — `WAIT_FOR_READY_OVERRIDE` thread-local + `WaitForReadyOverrideGuard` (RAII, nests cleanly). `wait_for_authoritative_actor_ready` reads the override and substitutes a `RetryBudget` with the longer timeout when set. Ops_log gains three new event lines for observability: `route_wait_for_ready_override` on entry, `route_wait_for_ready_elapsed` on success, `route_wait_for_ready_timeout` on bound exhaustion.
- `src/route/tests.rs` — new `wait_for_ready_override_guard_sets_and_restores_thread_local` test covers guard mechanics (set, nest, unset, restore). 3 existing `run_with_tmux` call sites updated for the new signature.

Verification:
- `cargo test --bin agent-doc wait_for_ready_override_guard` → 1 passed.
- `make check` (clippy -D warnings + full suite) → exit 0.
- `cargo build --release` + `cargo install --path .` → `~/.cargo/bin/agent-doc` v0.33.16 replaced.
- Selective stage in the submodule commit: only `src/main.rs` (my 22-line hunk extracted from a larger in-flight diff), `src/route.rs`, `src/route/tests.rs`. Other working-tree drift left untouched.

Plan file updated at `tasks/agent-doc/plan-jb-run-agent-doc-starting-actor-refusal.md` — Phase 1 marked `[done]` with implementation + test summary.

#### What's left on `#jbrunstart` (gated open)
- Phase 2 — JB plugin Run Agent Doc passes `--wait-for-ready 60` (or project-configurable) on user-initiated actions, with a Retry button on the IDE notification when the override still times out. Optional busy spinner for the wait.
- Phase 3 — SimWorld scenario: route while authoritative actor is `starting`, prompt becomes ready mid-wait, assert success without rerun; plus JB plugin test where feasible.

#### Queue
Next head: `do [#jbrsrbusysim]` (SimWorld + IDE telemetry for `session_restart --force`; plan phase 4 of `plan-jb-restart-supervisor-busy-refusal.md`; phases 1 and 3 already in `session_actor_cmd.rs`).
<!-- agent:boundary:36579ab6 -->
<!-- /agent:exchange -->
###
<!--
As I was typing into the comment below `/agent:exchange`, the full-document IPC corruption happened, then the duplicate line happened, then the full-document IPC corruption happened again.
#spec-test-build-install-commit-push
---
JB `Run Agent Doc` on docs.md had an error
```
resync: stash pane %23 is registered in its own project root — skipping kill
resync: stash pane %11 is registered in its own project root — skipping kill
resync: stash pane %28 is registered in its own project root — skipping kill
resync: stash pane %33 is registered in its own project root — skipping kill
resync: stash pane %13 is registered in its own project root — skipping kill
resync: stash pane %26 is registered in its own project root — skipping kill
[route] debounce OK — file idle for 22.1s and typing indicator idle
[route] target tmux session: 0
Error: authoritative actor generation 56 for tasks/docs.md owns pane %39 but route will not inject a new trigger because the authoritative actor is still starting (waited 10s for claude startup). Wait for the pane to show a dispatch-ready prompt (`prompt_ready=true`), then rerun `agent-doc tasks/docs.md`. If the pane stays stuck, restart the owner with `agent-doc start tasks/docs.md`.
```
#spec-test-build-install-commit-push
---
Please make auto-compact exchange off by default. Only explicit frontmatter or .agent-doc/config.toml setting should make auto-compact exchange active.
-->

<!-- agent:queue auto -->
preset #spec-test-build-install-commit-push
- do [#jbrsrbusysim]
- do [#jbccfix]
- do [#jbccreg]
- do [#jbccdocs]
<!-- /agent:queue -->

## Pending / Not Built

<!-- agent:backlog auto -->
- [ ] [#jbrsrbusysim] SimWorld + IDE telemetry coverage for session_restart --force path, including session_restart_busy_pre_interrupt_idle and session_restart_busy_force_killed event variants (plan phase 4). Plan: tasks/agent-doc/plan-jb-restart-supervisor-busy-refusal.md.
- [ ] [#jbccfix] Fix the Compact Exchange write/cache boundary so JetBrains never receives a stale or conflicting file-cache snapshot after compact exchange.
- [ ] [#jbccreg] Add deterministic regression coverage for Compact Exchange cache-conflict handling, including stale snapshot and concurrent user-edit cases where feasible.
- [ ] [#jbccdocs] Document the Compact Exchange File Cache Conflict recovery path in the JetBrains/harness runbooks after the behavior is fixed.
<!-- /agent:backlog -->

## Review

<!-- agent:review -->
- [/] [#jbrunstart] JB Run Agent Doc fails with starting-actor refusal after 10s wait when Claude Code startup takes longer (common after Restart Supervisor). Add --wait-for-ready <SECONDS> to route + JB plugin retry/spinner UX. Plan: tasks/agent-doc/plan-jb-run-agent-doc-starting-actor-refusal.md
- [/] [#ipcdupfinalize] IPC corruption + duplicate response when user types into a post-exchange comment during finalize. Socket-IPC ack=error falls through to file-IPC fallback that re-writes a response already in the live file, producing a duplicate response heading. Plan: tasks/agent-doc/plan-ipc-corruption-and-duplicate-during-typing.md
- [/] [#jbrsrbusy] [partial] Phase 1 (binary) landed: agent-doc session restart <FILE> --force flag accepts alive-busy panes, refusal message now ends with the --force hint citing the exact command, and the force path logs session_restart_force_used. Plan phases 2 (JB plugin confirm dialog → invokes --force), 3 (optional harness-aware pre-interrupt with 2s idle wait), and 4 (SimWorld coverage + IDE-side telemetry) split into #jbrsrbusyjb, #jbrsrbusyint, and #jbrsrbusysim. Plan: tasks/agent-doc/plan-jb-restart-supervisor-busy-refusal.md.
- [/] [#stuckcycledup] [recommended] [partial] Wedged-cycle pattern: cycle_already_committed gate now refuses the short-circuit when the active capture body is absent from HEAD (flow/closeout.rs::cycle_already_committed + new stuck_captured_cycle helper), and preflight emits a stuck_captured_cycle warning. Remaining plan parts split into #stuckcycleorphan, #stuckcyclerepair, and #stuckcyclesuperseded. Plan: tasks/agent-doc/plan-stuck-cycle-causes-duplicated-uncommitted-response.md.
- [/] [#jbresstart] Refuse cross-file pane rescue and extend Starting-actor wait after stash recovery; see tasks/agent-doc/plan-jb-route-rescued-stash-pane-starting-actor.md
<!-- /agent:review -->

<!-- agent:icebox -->
<!-- /agent:icebox -->

## Completed / Reaped

<!-- agent:done archive=tasks/agent-doc-bugs2.done.md -->

<!-- completed work archived in tasks/agent-doc-bugs2.done.md -->
<!-- /agent:done -->
