---
agent_doc_session: be20dd6e-31ce-4f16-93ed-42c9c2a21b98
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
  '#next-steps': ''
  '#spec-test-build-install-commit-push': update spec + tests. build + install for local testing. commit + push
  '#spec-test-commit-push': update spec + tests. commit + push
queue: stop
---

# agent-loop operations

Operational task document for agent-loop workspace management.

**History** (archived in git, commits up to `f80f524`):
- Submodule conversions (briantakita.me-dev, ctx-core-dev, mail, python-intentions)
- mail/mailboxes/lucas nested submodule init + push
- JetBrains module creation (20 .iml files)
- IDEA Cargo project path fixes (workspace.xml)
- Recursive `.agent-doc/` gitignore additions (20+ submodules)
- `.sync-state.json` gitignore for corky/mail
- Nested submodule fixes (ctx-core-dev/lib/relysjs, rappstack-dev/lib/ui--server--blog)

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/18c3ca3007b9c6c5bc21d81a0b9ff27f894487e97141f4cdafe2a9d12a4a19b1-20260625-183304.md`*

Compacted content:
- Archived 5 response topic(s): Commit + push workspace changes; Rebase agent-loop origin; Queue Preservation Repair; 2 more
- Prior summary/context: ❯ *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/18c3ca3007b9c6c5bc21d81a0b...

### Re: Session compaction — gpt-5

The compacted exchange is reconciled. I reviewed the archived response topics: the workspace commit/push, root rebase, and queue-preservation work were already completed; the JetBrains restart-delay report and inert `#advance-review` token were already captured as follow-up work.

No new operator request or uncompleted repository action was introduced by this compaction artifact, so no code, tests, builds, commits, or pushes were needed beyond this session-document closeout. The existing backlog remains intact, including `#2f3v` for the `#advance-review` normalization.
<!-- agent:boundary:857a0a1a:agent-loop -->
<!-- /agent:exchange -->
###
<!--
-->

## Queue

<!-- agent:queue -->
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog -->
- [ ] [#2f3v] Define or remove the unregistered `#advance-review` queue token in `tasks/software/agent-loop.md`; this document has no `agent:review` component and no local preset for that token.
- [ ] [#bx57] Disable unused MCP servers — currently managed via Claude Code UI, not settings.json
- [ ] [#wr52] Run litellm proxy pointing at vllm
- [ ] [#04hc] Test `claude` against local model via `ANTHROPIC_BASE_URL`
<!-- /agent:backlog -->

## Completed / Reaped

<!-- agent:done -->
- 2026-05-11 [#aaj9] Transfer snapshot GC feature to agent-doc-feat.md
- 2026-05-11 [#bamc] Evaluate graphify as graph layer for code RAG (extend extractors with tree-sitter code parsing)
- 2026-05-19 [#0b3x] [recommended] Create a composable-skill-design skill or extend skill-creator with a composability audit workflow, backed by examples and validation checks
- 2026-05-19 [#cdvx] [recommended] Check crates.io availability for candidate public crate names before renaming skill-harness
- 2026-05-19 [#25k6] [recommended] Keep skill-harness as the current Rust crate/package boundary, or rename only to an available precise crate name such as agent-skillkit/agent-skill-tools after checking registry availability; do not target the taken agent-skills crate name
- 2026-05-19 [#e92b] [recommended] Move compose-skills toward skill-harness/agent-skills with Rust validation and Claude/Codex installer coverage
- 2026-06-08 [#jq1r] corky workspace-split crates.io publish blocked on a topology decision (claim 5 new corky-* crate names + dependency-ordered publish, or keep binary/PyPI-only). Plan + recommendation: tasks/software/corky-workspace-publish.md
<!-- /agent:done -->
