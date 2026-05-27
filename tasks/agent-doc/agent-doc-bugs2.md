---
agent_doc_session: b26b9957-8de6-4eae-bb98-5d7fe4d6b781
agent: codex
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
  '#next-steps': Any follow-up items to place in the acklog? Please ensure the backlog is in priority order. Print the item id of the top of the backlog.
  '#spec-test-build-install-commit-push': update spec + tests. build + install for local testing. commit + push
  '#spec-test-commit-push': update spec + tests. commit + push
queue_active: true
---

## Status

<!-- agent:status patch=replace -->

Live IntelliJ File Cache Conflict Cancel proof captured; JB Cancel residual review items completed.
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7a9b103bd5b68223dac05a4c449ac03899aba53d6022e82100578b5b0-20260527-031115.md`*

Compacted content:
- Archived 2 response topic(s): log review and next steps; live queue IPC corruption
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7...
<!-- agent:boundary:3be9af0a -->
<!-- /agent:exchange -->
###
<!--
Please review the agent-doc, claude, codex logs. What are #next-steps to fix bugs?
---
When I ran the last turn, #spec-test-build-install-commit-push was removed by agent-doc.
---
As I was typing into the comment below `/agent:exchange`, the full-document IPC corruption happened, then the duplicate line happened, then the full-document IPC corruption happened again.
#spec-test-build-install-commit-push
-->

<!-- agent:queue auto -->
dispatch #spec-test-build-install-commit-push
- do [#ignorebypass]
- do [#liveipcrace]
- do [#snapabsorb]
- do [#compactstuck]
- do [#commentpreset]
- do [#cyclediag]
<!-- /agent:queue -->

## Pending / Not Built

<!-- agent:backlog -->
- [ ] [#ignorebypass] Teach the finalize/write/commit auto-sweep to honor `.gitignore`. Today it stages via `git hash-object` + `git update-index --add` (porcelain-bypassing plumbing), so any file the session document references gets committed regardless of ignore rules. Concrete repro: with `tasks/.gitignore` listing `scratch/`, an agent-doc session that references `tasks/scratch/<x>.md` commits that file silently on finalize. Fix surface: pre-stage check via `git check-ignore --quiet --no-index` (or libgit2 equivalent) per candidate path before `update-index`; skip ignored paths with a visible diagnostic (or behind an `--include-ignored` opt-in) so users don't have to rebase-force-push to undo. Bug class: similar to #snapabsorb in that plumbing-level staging skips the safety net users expect from porcelain.
- [ ] [#liveipcrace] Reproduce and fix the live post-exchange typing/full-document IPC corruption path: build a failing SimWorld plus JetBrains/VSC fixture from cycle-1779845677327 where a scratch comment is edited during finalize/compact, assert no fullContent socket/file payload, no duplicate prompt line, no prompt preset/directive loss, and no silent snapshot_absorb of shorter disk content.
- [ ] [#snapabsorb] Harden write/commit closeout after ipc_snapshot_adoption_blocked: when live_prompt_drift_after_preflight selects content_ours for snapshot, later status/commit staging must not snapshot_absorb a divergent disk body unless the response body and live drift are both proven materialized.
- [ ] [#compactstuck] Teach stuck_captured_cycle/preflight that compact exchange archives are valid materialization for committed captures, or retire the capture/cycle state during compact, so archived responses do not warn as missing from HEAD.
- [ ] [#commentpreset] Define the contract for prompt-preset/directive text inside ordinary post-exchange HTML comments: either keep it non-executable but emit a visible diagnostic, or introduce a draft-prompt component so #spec-test-build-install-commit-push cannot look silently removed.
- [ ] [#cyclediag] Add an agent-doc cycle diagnosis command/report that gathers ops.log, capture JSON, route/actor events, Codex session logs, Claude session logs, and editor plugin logs by cycle_id/patch_id to make future race reports reproducible without broad rg searches.
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
