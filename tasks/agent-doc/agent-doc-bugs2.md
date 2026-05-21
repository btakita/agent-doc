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

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7a9b103bd5b68223dac05a4c449ac03899aba53d6022e82100578b5b0-20260521-050730.md`*

Compacted content:
- Archived 1 response topic(s): graph acceptance gate
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7...
### Re: job packet graph gate — gpt-5

Done: `#w25k`, `#aggtp`, `#jpktscope`, and `#jpktwsmis` are implemented and marked complete.

For `#w25k`, I repaired the local tsift graph-db blocker by quarantining the hot `graph.db` and journal files, then refreshed the graph. `tsift graph-db status` is current and `tsift graph-db doctor` reports `ok`, so the read-only hot-journal state is cleared.

For `#aggtp`, I added the graph-backed acceptance gate around orchestration/job packet creation and collection. Planning now includes graph evidence for queue-driven `do` targets, worker packet evidence is validated for packet id, projection hash, expansion commands, token budget, and worker feedback, and worker results now fail closed unless they include the expected touched files, expected tests, follow-up ids, changed paths, commands, proof, findings, confidence, and status contract. I also bounded tsift graph subprocesses with `AGENT_DOC_TSIFT_GRAPH_TIMEOUT_SECS` so an expensive graph query fails closed instead of hanging the cycle.

For `#jpktscope`, `jobs create` now expands the union of pending mutations plus all ids from compound `do` directives, so directives like `do [#x63e] [#v4v0]` create one packet per target instead of only the first one.

For `#jpktwsmis`, job packet write scopes are now target-specific. `jobs create` prefers explicit `scope:`, `write_scope:`, or `write-scope:` path refs from each backlog item, then falls back to path-like refs in the item text, and only then uses the broader plan scope.

Verification:

- `tsift graph-db refresh`, `status`, and `doctor` passed after repair.
- Focused coverage passed for job packets, compound `do` planning, graph target parsing, and the tempdir audit regression.
- `make check` passed.
- Latest GitHub CI run `26206558029` had failed at `Run make check` before reaching tmux; local `make check` is now green.
- `make tmux-ci` passed locally, including 228 ignored/live tmux tests in the main binary suite.
- `cargo install --path .` completed and the installed binary reports `agent-doc 0.33.16`.

Committed:

- `src/agent-doc`: `9f06a014 feat: tighten job packet graph gate`
- superproject pointer: `8173512dd chore: update agent-doc submodule`

Per the session commit boundary, I am pushing those commits immediately after this document closeout commit lands.
<!-- agent:boundary:7cefab95 -->
<!-- /agent:exchange -->
###
<!--
The duplicate content corrupting document and duplicate prompt issues happened yet again. Very tired of playing whack-a-mole. Reproduce bugs with tests first that fail and fix the implementation. #spec-test-build-install-commit-push
-->


<!-- agent:queue -->
preset #spec-test-build-install-commit-push
- do [#w25k]
- do [#aggtp]
- do [#jpktscope]
- do [#jpktwsmis]
<!-- /agent:queue -->

## Pending / Not Built

<!-- agent:backlog -->
<!-- /agent:backlog -->

<!-- agent:icebox -->
<!-- /agent:icebox -->

## Completed / Reaped

<!-- agent:done archive=tasks/agent-doc-bugs2.done.md -->

<!-- completed work archived in tasks/agent-doc-bugs2.done.md -->
<!-- /agent:done -->
