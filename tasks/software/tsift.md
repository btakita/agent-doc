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
queue: stop
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

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc65443c45f7b49f5e6ace54e3f6bafe4077ade57924422b5eb3baf4dbd23c-20260609-175219.md`*

Compacted content:
- Archived 2 response topic(s): pcache-measure - gpt-5; pcache-fingerprint - gpt-5
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc6544...

### Re: prompt caching backlog — gpt-5

Added prompt-caching follow-up backlog items in priority order. Top backlog item: `#pcache-replaygate`.

No code/test run was needed because this cycle only mutates the session backlog.
<!-- agent:boundary:5b8ab757 -->
<!-- /agent:exchange -->
###
<!--
What are #next-steps to promote SurrealDB to SQLite-equivalent substrate status?
-->

## Queue

<!-- agent:queue preset="#spec-test-build-install-commit-push" go -->
 - Add backlog items to improve prompt caching.
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog priority queue -->
- [ ] [#pcache-replaygate] Prompt caching: add a real consecutive-turn replay gate for agent-doc sessions that proves stable-prefix fingerprints, provider cache keys/cache_control, and routing affinity stay constant across no-op/status/queue churn, then intentionally invalidates on durable instruction changes.
- [ ] [#pcache-missrank] Prompt caching: add cache-miss ranking to session-cost output by fingerprint, adapter state, routing affinity, cached-input delta, and creation-token spike so the highest-impact invalidation cause is visible without JSON analysis.
- [ ] [#pcache-boundary-contract] Prompt caching: document and test the cache-boundary contract for agent-doc prompt assembly: which blocks may live above the boundary, which must stay in the volatile suffix, and provider-specific breakpoint/key requirements.
- [ ] [#pcache-ci-history] Prompt caching: persist prompt-cache effectiveness history and trend checks in the token/performance gate so regressions across real Codex/OpenAI and Claude/Anthropic transcripts fail with actionable deltas.
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
