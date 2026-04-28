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
  '#code-review': Please review agent-doc codebase. Inspect the architecture and look for edge cases. Isolate concurrency issues and other workflow bugs. Let's find root causes to fix. '#follow-up-backlog'
  '#commit-push': commit + push all uncommitted files
  '#next-steps': Any follow-up items to place in the backlog? Please ensure the backlog is in priority order. Print the item id of the top of the backlog.
  "#spec-test-build-install-commit-push": update spec + tests. build + install for local testing. commit + push
  "#spec-test-commit-push": update spec + tests. commit + push
---

## Status

<!-- agent:status patch=replace -->
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
Compacted exchange summary: this session tracked agent-doc workflow bugs across patchback/commit-boundary recovery, tmux route/start/resync ownership, startup-miss and routed-cycle acknowledgment, backlog shadow/replay guards, compaction dispatch, tail-patch fail-closed behavior, and prompt-diff visibility. Implemented fixes were pushed across src/agent-doc, most recently #patchbypass () for exchange-only recovery of missing commit boundaries. Key outcomes: route/start now health-check or reuse live panes instead of spawning/killing duplicates; resync preserves correctly bound live sessions; closeout paths share strict commit/session-check enforcement; backlog/history guards fail closed when work drifts out of tracked components; and explicit-baseline writes keep concurrent user prompts visible in the next diff. No open backlog items remain in this document and no unanswered user prompt remained beyond the compaction request.
<!-- agent:boundary:fe3a4909 -->
<!-- /agent:exchange -->

<!--
-->

## Pending / Not Built

<!-- agent:backlog -->
<!-- /agent:backlog -->
