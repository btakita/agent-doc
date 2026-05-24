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

Resolved #qfixture; auto queue drained.
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7a9b103bd5b68223dac05a4c449ac03899aba53d6022e82100578b5b0-20260524-222126.md`*

Compacted content:
- Archived 3 response topic(s): next steps; jbccache; queue stall
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7...
### Re: JB Restart Supervisor + Run Agent Doc errors — opus-4-7

Captured both errors as `#agent-doc-bug` plans + backlog items. Deferring the queue's
`dispatch #spec-test-build-install-commit-push` + `do [#jbccache|fix|reg|docs]` to a
subsequent cycle — those need code work, build, install, and live verification per
release discipline.

#### Bug 1 — JB Restart Supervisor refused (alive-busy)

`session_restart` shares the alive-busy gate with passive transitions, so the user's
explicit "kick this loose" gesture fails closed. Plan
`tasks/agent-doc/plan-jb-restart-supervisor-busy-refusal.md` covers:

- `--force` on `session_restart` for explicit-intent callers; refusal message gains a
  `--force` hint.
- JB Restart Supervisor action prompts ("Interrupt and restart?") and invokes the
  force path on confirm.
- Optional harness-aware pre-interrupt (Ctrl-C / cancel) to reach idle before kill.
- Distinct ops-log events + SimWorld regression covering force/non-force paths.

Backlog id: `#jbrsrbusy`.

#### Bug 2 — JB Run Agent Doc failed after stash rescue (actor Starting)

Two issues in one trace — the rescued pane `%33` is owned by `tagpath.md` while the
route target was `tsift.md` (cross-file leak), and the 8s codex startup wait is too
short after a fresh rescue. Plan
`tasks/agent-doc/plan-jb-route-rescued-stash-pane-starting-actor.md` covers:

- Route gate refuses cross-file rescue before mutating pane topology; emits
  `route_cross_file_rescue_refused`.
- After legitimate rescue, treat the actor as fresh-Starting and apply the
  harness-aware budget from `plan-route-starting-actor-race-after-restart.md`.
- Rescue diagnostics (`source_window`, `target_window`, `owning_file`) on the
  "still starting" error.
- SimWorld coverage for cross-file refusal + extended wait paths.

Backlog id: `#jbresstart`.

#### Deferred queue items

`#jbccache` is referenced in the queue but absent from backlog — leaving as-is rather
than auto-promoting; please confirm whether to add a plan or drop the directive.
`#jbccfix`, `#jbccreg`, `#jbccdocs` remain in backlog and will be picked up under
`#spec-test-build-install-commit-push` in a follow-up turn.
<!-- agent:boundary:677df467 -->
<!-- /agent:exchange -->
###
<!--
-->

<!-- agent:queue auto -->
preset #spec-test-build-install-commit-push
- do [#jbresstart]
- do [#jbrsrbusy]
- do [#jbccfix]
- do [#jbccreg]
- do [#jbccdocs]
<!-- /agent:queue -->

## Pending / Not Built

<!-- agent:backlog -->
- [ ] [#jbresstart] Refuse cross-file pane rescue and extend Starting-actor wait after stash recovery; see tasks/agent-doc/plan-jb-route-rescued-stash-pane-starting-actor.md
- [ ] [#jbrsrbusy] Fix JB Restart Supervisor alive-busy refusal — add --force + JB confirm + harness pre-interrupt; see tasks/agent-doc/plan-jb-restart-supervisor-busy-refusal.md
- [ ] [#jbccfix] Fix the Compact Exchange write/cache boundary so JetBrains never receives a stale or conflicting file-cache snapshot after compact exchange.
- [ ] [#jbccreg] Add deterministic regression coverage for Compact Exchange cache-conflict handling, including stale snapshot and concurrent user-edit cases where feasible.
- [ ] [#jbccdocs] Document the Compact Exchange File Cache Conflict recovery path in the JetBrains/harness runbooks after the behavior is fixed.
<!-- /agent:backlog -->

<!-- agent:icebox -->
<!-- /agent:icebox -->

## Completed / Reaped

<!-- agent:done archive=tasks/agent-doc-bugs2.done.md -->

<!-- completed work archived in tasks/agent-doc-bugs2.done.md -->
<!-- /agent:done -->
