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
- [ ] [#8zjh] [recommended] Validate the live routed rerun for the completed-turn stale-boundary repair in the original JB/tmux repro
- [ ] [#wcrp] Repeated-response repair gap: if a session doc contains a visible `### Re:` block paired with a bare `do #...` prompt instead of `❯ do #...`, `agent-doc session-check`, `preflight`, `repair`, and `run` can all fail closed on the same typed-component-drift guard with no automatic recovery. Repro from `src/boost-client/tasks/monsterrodholders.md` on April 30-May 1, 2026 around the repeated `#wcup1` exchange. Fix should canonicalize prompt/response markers during adopt-current/repair or provide a deterministic one-shot repair path that does not require manual document surgery.
- 1 more active item(s)

Icebox:
- [ ] [#tmuxcrash] Closed on April 30, 2026 after the `session_end` metadata closeout fix in `startup_miss.rs` and the routed-dispatch binding hardening in `route.rs`. Final child closed: `#tmuxcrash-rn2d`.
<!-- agent:boundary:6168cae9 -->
<!-- /agent:exchange -->

<!--
Can we isolate the parallel tmux test from each other?
> •  The serial full-suite run is underway. I switched to --test-threads=1 because the earlier failure set was all tmux/
  session tests that pass in isolation and are clearly contending with each other, which would otherwise blur whether
  this change actually regressed behavior.
---
If I restart codex with the supervisor, the real-time the next session process has a truncated feed at the bottom.
![img_7.png](img_7.png)
-->

## Pending / Not Built

<!-- agent:backlog -->
- [ ] [#6btt] Committed closeout parity gap for src/session-share/tasks/claudescore-2.md: a cycle can report committed after snapshot-side repair while the visible worktree/CRDT copy still carries unresolved prompt-bearing drift such as bare 'compact exchange', '(HEAD)' response heading drift, or newer boundary state. Repro from May 1, 2026. Fix should prove HEAD/snapshot/live-file parity before success or reopen the same turn automatically. Plan: tasks/agent-doc/plan-claudescore-2-compact-closeout-drift.md
- [ ] [#8zjh] [recommended] Validate the live routed rerun for the completed-turn stale-boundary repair in the original JB/tmux repro
- [ ] [#wcrp] Repeated-response repair gap: if a session doc contains a visible `### Re:` block paired with a bare `do #...` prompt instead of `❯ do #...`, `agent-doc session-check`, `preflight`, `repair`, and `run` can all fail closed on the same typed-component-drift guard with no automatic recovery. Repro from `src/boost-client/tasks/monsterrodholders.md` on April 30-May 1, 2026 around the repeated `#wcup1` exchange. Fix should canonicalize prompt/response markers during adopt-current/repair or provide a deterministic one-shot repair path that does not require manual document surgery.
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
