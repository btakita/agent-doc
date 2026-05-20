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
Completed stale visible-closeout write guard and added the IPC editor-side proof follow-up. Top backlog item: #ipcw.
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7a9b103bd5b68223dac05a4c449ac03899aba53d6022e82100578b5b0-20260520-183109.md`*

Compacted content:
- Archived 5 response topic(s): editor IPC apply proof; #nh3j status; #nh3j audit-docs root scope; 2 more
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7...
❯ do [#ops2]. #spec-test-build-install-commit-push
### Re: #ops2 FlowCore clear and orchestration — gpt-5

Implemented `#ops2` in `src/agent-doc`.

Changes:
- Moved `session clear` guard decisions onto `flow::operator_clear`, including typed outcomes for idle, clean-exit, protected-input, active agent pane, busy pane, and no-live-pane cases.
- Changed file-scoped clear to fail closed on live busy agent/harness panes and point operators to `agent-doc session interrupt-clear` for destructive discard.
- Moved sequential orchestration batch freeze, mid-run source mutation, child closeout, and child patchback normalization proof onto `flow::orchestration_batch` events.
- Kept child plain-response wrapping on the shared document-mutation patchback contract, with transcript/full-document/malformed shapes left rejected.
- Updated session/orchestration specs and ops summary grouping for the new typed events.

Verification:
- `cargo test operator_clear -- --test-threads=2`
- `cargo test orchestration_batch -- --test-threads=2`
- `cargo test sequential_orchestration_stops_when_exchange_task_list_changes -- --test-threads=2`
- `cargo test orchestrate_template_finalize -- --test-threads=2`
- `make check`
- `make install`
- `agent-doc --version` -> `agent-doc 0.33.16`
- Latest upstream CI run `26181953001` was green, including `Run make tmux-ci`.

Commits:
- `btakita/agent-doc` `c3608203` (`feat: route clear and orchestration through FlowCore`) pushed to `20260422/codex-agent-backend`.
- Parent gitlink commit `956dfa3e4` (`chore: update agent-doc FlowCore submodule`) is staged for the parent push after this document closeout commits.
<!-- agent:boundary:0cfd044c -->
<!-- /agent:exchange -->
###
<!--
-->

<!-- agent:queue -->
dispatch #spec-test-build-install-commit-push
<!-- /agent:queue -->

## Pending / Not Built

<!-- agent:backlog -->
- [ ] [#y9kk] [recommended] Phase 1 for lower-agent orchestration: define job packet + worker result schemas, add the manual job-packet runbook, and include examples for search, test triage, small patch, and spec update jobs. Plan: tasks/agent-doc/plan-lower-agent-orchestration-token-savings.md
- [ ] [#v44k] [recommended] Phase 2 for lower-agent orchestration: add tsift JSON contracts for context-pack, source-read, diff-digest, test-digest, index-status, stable handles, freshness checks, and token estimates. Plan: tasks/agent-doc/plan-lower-agent-orchestration-token-savings.md
- [ ] [#ffsm] [recommended] Phase 3 for lower-agent orchestration: extend agent-doc plan with task_class, dispatch_candidate, model tier, context budget, write scope, required proof, and tsift missing/stale fallback behavior. Plan: tasks/agent-doc/plan-lower-agent-orchestration-token-savings.md
- [ ] [#rafe] [recommended] Phase 4 for lower-agent orchestration: implement agent-doc jobs create/list/status/collect plus optional operation-doc generation and closeout links. Plan: tasks/agent-doc/plan-lower-agent-orchestration-token-savings.md
- [ ] [#ek86] Implement the lower-agent orchestration MVP from tasks/agent-doc/plan-lower-agent-orchestration-token-savings.md, starting with manual job packets plus tsift context packets.
- [ ] [#fwjw] Define the agent-doc taxonomy for agent docs, job packets, operation docs, runbooks, and plans.
- [ ] [#0fcx] Specify operation docs/opdocs as retained audit artifacts and how they relate to job packets.
<!-- /agent:backlog -->

<!-- agent:icebox -->
<!-- /agent:icebox -->

## Completed / Reaped

<!-- agent:done archive=tasks/agent-doc-bugs2.done.md -->

<!-- completed work archived in tasks/agent-doc-bugs2.done.md -->
<!-- /agent:done -->
