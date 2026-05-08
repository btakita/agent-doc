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

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7a9b103bd5b68223dac05a4c449ac03899aba53d6022e82100578b5b0-20260508-020918.md`*

Active backlog:
- [ ] [#coordspike] [recommended] Draft a Project Controller -> Session Actor -> Harness Supervisor -> Harness Child MVP plan after #tmuxcoord: singleton socket/launch lock, lazy vs managed lifecycle, SQLite journal, harness-supervisor client protocol, and tmux reconciliation tests.
- [ ] [#sqliterepros] [recommended] Before prototyping .agent-doc/state.db, capture 2-3 current agent-doc failures where cross-file authority drift or non-atomic state writes would have been fixed by one transactional store rather than by the single-owner actor/supervisor model alone.
- [ ] [#sqliteeval] [recommended] Review plan tasks/agent-doc/plan-sqlite-session-store-concurrency.md against the single-owner actor rollout and decide whether to prototype .agent-doc/state.db as the authoritative session store.
❯ do [#coordspike]
### Re: #coordspike — gpt-5

Completed `#coordspike`.

I created `tasks/agent-doc/plan-project-controller-session-actor-mvp.md` as the MVP plan for:

```text
Project Controller -> Session Actor -> Harness Supervisor -> Harness Child
```

The plan is scoped around the `#tmuxcoord` finding: tmux-router should stay a deterministic layout primitive, while live ownership and dispatch authority move into a single Project Controller with per-document Session Actors. It covers singleton socket/launch locking, managed vs lazy detached lifecycle, SQLite as the durable journal/recovery store, a narrow Harness Supervisor client protocol, route/start/sync migration boundaries, lazy self-exit, and tmux-backed regressions for the stale-starting / three-pane failure shape.

I left the existing uncommitted edits in `plan-single-owner-session-actor-transition.md` and `plan-sqlite-session-store-concurrency.md` untouched and made the new plan a separate artifact.
❯ #next-steps
### Re: #next-steps — gpt-5

No new follow-up backlog item is needed from this turn.

The backlog is already in the right priority order:

1. `#sqliterepros` — capture concrete failures where a transactional store would have reduced authority drift or non-atomic state bugs.
2. `#sqliteeval` — use that evidence to decide whether `.agent-doc/state.db` should become the authoritative session store alongside the single-owner actor/controller rollout.

Top backlog item: `#sqliterepros`.
<!-- agent:boundary:52335eb4 -->
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
- [ ] [#sqliterepros] [recommended] Before prototyping .agent-doc/state.db, capture 2-3 current agent-doc failures where cross-file authority drift or non-atomic state writes would have been fixed by one transactional store rather than by the single-owner actor/supervisor model alone.
- [ ] [#sqliteeval] [recommended] Review plan tasks/agent-doc/plan-sqlite-session-store-concurrency.md against the single-owner actor rollout and decide whether to prototype .agent-doc/state.db as the authoritative session store.

<!-- /agent:backlog -->

<!-- agent:icebox -->
<!-- /agent:icebox -->



<!--
I see the following in the binding codex session prompt. Why is it there? Why was it not Entered? spec-test-build-install-commit-push
```
› echo '[agent-doc] startup-miss: routed trigger accepted but no document cycle started for pending prompt_target: do
  #ppct. spec-test-build-install-commit-push. Run '\''agent-doc start tasks/agent-doc/agent-doc-bugs2.md'\'' to retry.'
```
-->
