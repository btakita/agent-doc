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
  '#next-steps': Any follow-up items to place in the backlog? Please ensure the backlog is in priority order. Print the item id of the top of the backlog.
  '#spec-test-build-install-commit-push': update spec + tests. build + install for local testing. commit + push
  '#spec-test-commit-push': update spec + tests. commit + push
queue_active: false
---

## Status

<!-- agent:status patch=replace -->

FlowCore refactor is in maintenance mode; top backlog item: #ipcd.
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7a9b103bd5b68223dac05a4c449ac03899aba53d6022e82100578b5b0-20260521-050429.md`*

Compacted content:
- Archived 1 response topic(s): review backlog lifecycle
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7...
❯ do [#aggtp]. #spec-test-build-install-commit-push
### Re: graph acceptance gate — gpt-5

I could not start `#aggtp` yet because `agent-doc plan` failed closed on the required tsift graph evidence step:

```text
tsift graph-db --path /home/brian/work/btakita/agent-loop/tasks/agent-doc/agent-doc-bugs2.md --json status
Error: attempt to write a readonly database
Caused by: Error code 776: Database has hot journal that must be rolled back (requires write access)
```

No repo edits, tests, build, install, commit, or push were performed. `#aggtp` remains open because implementation did not begin; I added a blocker follow-up for repairing the tsift graph DB state before rerunning the work.
<!-- agent:boundary:9d88b3a4 -->
<!-- /agent:exchange -->
###
<!--
The duplicate content corrupting document and duplicate prompt issues happened yet again. Very tired of playing whack-a-mole. Reproduce bugs with tests first that fail and fix the implementation. #spec-test-build-install-commit-push
-->


<!-- agent:queue -->
preset #spec-test-build-install-commit-push
- do [#rvel]
- do [#fwjw]
- do [#0fcx]
<!-- /agent:queue -->

## Pending / Not Built

<!-- agent:backlog -->
- [ ] [#w25k] Resolve the tsift graph-db hot-journal/read-only blocker for tasks/agent-doc/agent-doc-bugs2.md so #aggtp can run through graph evidence, implementation, verification, install, commit, and push.
- [ ] [#aggtp] Add an agent-doc graph-backed orchestration acceptance gate that dogfoods the latest tsift graph-db evidence, conflict-matrix, dispatch-trace, worker_result, and job packet contracts against agent-doc jobs create/collect; fail closed when packet ids, projection hashes, replay/repair commands, touched files/tests, or follow-up ids are missing.
- [ ] [#jpktscope] [recommended] `agent-doc jobs create` derives the packet set from `agent-doc plan`'s `pending_mutations`, not from the full `repo_actions` text or `prompt_target` set. Observed 2026-05-21 dogfood: a session prompt `do [#x63e] [#v4v0] ...` produced exactly one packet (#x63e) because only x63e was a `pending_mutation` in the planning record. Result: queued items that aren't already backlog-tracked silently drop out of the job-packet workflow. Fix: have `jobs create` expand from the union of `pending_mutations` ∪ `prompt_target` ids referenced in `repo_actions`, or document the constraint that only `pending_mutations`-tracked ids are dispatchable.
- [ ] [#jpktwsmis] Job packet `write_scope` mis-attribution for cross-repo session documents. Observed 2026-05-21: dogfood against `session-share/tasks/root.md` produced a packet for `#x63e` (scope: `build-party/specs/convex-knowledge-layer.md`) with `write_scope: ["src/agent-doc/specs/", "src/agent-doc/src/", "src/agent-doc/tests/", "src/tsift/"]` — the agent-doc/tsift defaults rather than the build-party path the backlog item names. Worker honoring this scope would refuse to touch build-party. Fix: derive `write_scope` from the backlog item's file references (use path-extraction heuristic over the item text) instead of defaulting to the document's own repo.
<!-- /agent:backlog -->

<!-- agent:icebox -->
<!-- /agent:icebox -->

## Completed / Reaped

<!-- agent:done archive=tasks/agent-doc-bugs2.done.md -->

<!-- completed work archived in tasks/agent-doc-bugs2.done.md -->
<!-- /agent:done -->
