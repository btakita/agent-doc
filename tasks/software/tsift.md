---
agent_doc_session: tsift-v0.1
agent: codex
claude_model: opus
agent_doc_format: template
agent_doc_write: crdt
claude_args: --dangerously-skip-permissions
codex_args: -s danger-full-access
prompt_presets:
  '#agent-doc-bug': Please create a plan for agent-doc to fix this issue. Add to the backlog of agent-loop/tasks/agent-doc/agent-doc-bugs2.md
  '#code-review': Please review tsift codebase. Inspect the architecture and look for edge cases. Isolate concurrency issues and other workflow bugs. Let's find root causes to fix. '#follow-up-backlog'
  '#mermaid': Include explanatory mermaid diagram(s).
  '#next-steps': Any follow-up items to place in the backlog? Please ensure the backlog is in priority order. Print the item id of the top of the backlog.
  '#spec-test-build-install-commit-push': update spec + tests. build + install for local testing. commit + push
  '#spec-test-commit-push': update spec + tests. commit + push
queue: start
---

## Status

<!-- agent:status patch=replace -->

complete
<!-- /agent:status -->

## Spec

See [src/tsift/SPEC.md](../../src/tsift/SPEC.md) for the AST-Aware Code RAG spec.

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc65443c45f7b49f5e6ace54e3f6bafe4077ade57924422b5eb3baf4dbd23c-20260610-013837.md`*

Compacted content:
- Archived 1 response topic(s): next-steps
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc6544...
<!-- agent:boundary:539827c3 -->
<!-- /agent:exchange -->
###
<!--
What are #next-steps to promote SurrealDB to SQLite-equivalent substrate status?
-->

## Queue

<!-- agent:queue preset="#spec-test-build-install-commit-push" go -->
- ~~[#pcache-prefix-drift]~~
- [#contextpack-queue-slim]
- [#token-actions-rewrite]
- [#pcache-fixture-drift]
- [#pcache-scorecard]
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog priority queue -->
- [ ] [#contextpack-queue-slim] Add a queue-head context-pack profile for agent-doc documents: emit the active queue prompt, live unresolved exchange tail, top backlog/review rows, compact prompt preset references, and expansion handles while suppressing archives, done history, and stale session summaries by default.
- [ ] [#token-actions-rewrite] Promote session-review next_token_actions into exact rewrite commands for token-heavy behavior: repeated raw source/session/log reads, repeated command bundles, oversized prompt turns, and high cached-resend ratios should map to concrete tsift rewrite/source-read/session-review/context-pack commands.
- [ ] [#pcache-fixture-drift] Extend real-session prompt-cache fixtures with regression cases for volatile timestamps/generated headers in cached prefixes, cold standalone compaction, OpenAI prompt_cache_key churn, and replica routing churn; fail under when cache reads no longer beat creation work.
- [ ] [#pcache-scorecard] Add a compact prompt-cache ROI scorecard to session-cost/session-review: per provider/session show net cached-read tokens, read/create ratio, trend, suspected invalidation cause, and the one next command to run before the next long agent-doc cycle.
<!-- /agent:backlog -->

## Review

<!-- agent:review -->
<!-- /agent:review -->
###
<!--
Can we increase max-hop caps?
Could FalkorDB help with performance?
-->

<!-- agent:icebox -->
<!-- /agent:icebox -->

## Completed / Reaped

<!-- agent:done archive=tasks/software/tsift.done.md -->

<!-- completed work archived in tasks/software/tsift.done.md -->
<!-- /agent:done -->
