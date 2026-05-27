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
  '#next-steps': Any follow-up items to place in the backlog? Please ensure the backlog is in priority order. Print the item id of the top of the backlog.
  '#spec-test-build-install-commit-push': update spec + tests. build + install for local testing. commit + push
  '#spec-test-commit-push': update spec + tests. commit + push
queue_active: false
---

## Status

<!-- agent:status patch=replace -->
Added prioritized graph/neighborhood next steps to the tsift backlog, tagged for neighborhood effectiveness and performance. Top backlog item: #gneighqual.
<!-- /agent:status -->

## Spec

See [src/tsift/SPEC.md](../../src/tsift/SPEC.md) for the AST-Aware Code RAG spec.

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc65443c45f7b49f5e6ace54e3f6bafe4077ade57924422b5eb3baf4dbd23c-20260527-045155.md`*

Compacted content:
- Archived 6 response topic(s): #gneighgate; closeout repair; #gsemrows; 3 more
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc6544...

❯ Please add #next-steps for graph and tag neighborhood effectiveness and performance.
### Re: graph + neighborhood next steps — gpt-5

Added the requested `#next-steps` into the backlog in priority order, tagged across graph, neighborhood effectiveness, and performance.

Top backlog item: `#gneighqual`.

Priority shape:

1. `#gneighqual` — prove ranked-neighborhood effectiveness before changing defaults.
2. `#gcommqual` — improve tagpath/community context that feeds neighborhood ranking.
3. `#gfullhot` — reduce full-projection graph hot-path costs before backend changes.
4. `#gsqlstage` — reduce SQLite refresh staging/write costs.
5. `#ghoppolicy` — keep higher hop caps behind useful-row and latency evidence.
6. `#gbackendproof` — keep FalkorDB/Kuzu promotion behind native-adapter parity and performance proof.

No source code changed this cycle.
<!-- agent:boundary:d6f89b43 -->
<!-- /agent:exchange -->
###
<!--
What are #next-steps to make the graph faster?
---
What are #next-steps to make the graph + neighborhoods faster + more effective?
-->

<!-- agent:queue -->
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog -->
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
