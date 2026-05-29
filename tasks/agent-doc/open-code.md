---
agent_doc_session: 7910a62f-ef5f-4f02-9703-dade1f077521
agent: claude
claude_model: opus
opencode_model: GLM-5.1
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
queue_active: false
---

## Status

<!-- agent:status patch=replace -->
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/fb8de777e65629bf156ef66517a44d0d9c829c7e89b02b868f8ca9d0519d3ddc-20260529-064430.md`*

Compacted content:
- Archived 6 response topic(s): do [#adoc-sqlite-isolation]; do [#adoc-sqlite-seam]; do [#adoc-sqlite-seam]; 3 more
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/fb8de777...
<!-- agent:boundary:76b9d3e2 -->
<!-- /agent:exchange -->

## Queue

<!-- agent:queue -->
preset #spec-test-build-install-commit-push
- ~do [#adoc-sqlite-isolation]~
- ~do [#adoc-sqlite-seam]~
- do [#adoc-orch-shim-cleanup]
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog -->
- [ ] [#adoc-orch-shim-cleanup] Finish the migration: rewrite CLI-shell crate::X call sites to agent_doc_orchestration::X and delete the pub use shims in agent-doc/src/{lib,main}.rs. Mechanical/low-risk polish, not a true crate split. Plan's optional final step.
<!-- /agent:backlog -->

## Icebox

<!-- agent:icebox -->
<!-- /agent:icebox -->

## Completed / Reaped

<!-- agent:done archive=tasks/agent-doc/open-code.done.md -->

<!-- completed work archived in tasks/agent-doc/open-code.done.md -->
<!-- /agent:done -->
