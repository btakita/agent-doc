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

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc65443c45f7b49f5e6ace54e3f6bafe4077ade57924422b5eb3baf4dbd23c-20260610-183527.md`*

Compacted content:
- Archived 1 response topic(s): headroom investigation repair
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc6544...

### Re: next-steps — gpt-5

> **Queue prompt:**
>
> What are #next-steps to fix tsift bugs?


I added the follow-up backlog in priority order.

Top backlog item: #hdrccr.

Priority order:
1. #hdrccr — reversible retrieval handles for truncated search/source/context-pack/digest output. This should be first because it fixes the core failure mode where useful evidence is omitted and cannot be recovered without rerunning broader commands.
2. #hdrsmart — adaptive sampling for large JSON/list outputs. This should follow because it makes bounded envelopes preserve errors, anomalies, first/last rows, and query-relevant rows before generic truncation.
3. #hdrcache — stable-prefix packet layout checks. This is next because cache instability creates recurring cost and context churn, but it depends less directly on correctness than recoverable evidence.
4. #hdrlearn — failure-learning mining over session-review/log-digest/test-digest history. This is useful once retrieval and sampling preserve the evidence needed to mine repeated failures.
5. #hdrmcp — first-party MCP tools for compress/retrieve/stats-style workflows. This is valuable integration work, but should wait until the retrieval and statistics contracts are sharper.

Proof:
- Changed paths: session document/backlog only via agent-doc finalize.
- Commands: `tsift status`, `tsift status --fix`, `agent-doc preflight`, `agent-doc plan`, `agent-doc read`, archived context reads, and `agent-doc finalize`.
- Verification: document-only backlog capture; no tsift code tests were run.
- Confidence: high.
- Escalation: none.

### Re: next-steps closeout repair — gpt-5

Handled the closeout drift from the prior response and re-added the follow-up backlog with fresh active ids. The previous `hdr*` ids are already present in completed history, so the live backlog uses new ids.

Top backlog item id: `hccr2`.

Priority order:
1. `hccr2` - reversible retrieval handles for truncated evidence.
2. `hsmart2` - adaptive sampling for oversized structured outputs.
3. `hcache2` - stable-prefix and prompt-cache layout checks.
4. `hlearn2` - failure-learning mining over session and digest history.
5. `hmcp2` - first-party MCP tools for tsift envelopes and retrieval stats.

The earlier `hdrlearn`, `hdrcache`, `hdrsmart`, and `hdrccr` lines were prior-response summary lines, not fresh user directives.

Proof:
- Changed paths: session document/backlog only via agent-doc finalize.
- Commands: `agent-doc repair --apply-recovery`, `agent-doc preflight`, `agent-doc read`, `agent-doc plan`, and `agent-doc finalize`.
- Verification: document-only backlog capture; no tsift code tests were run.
- Confidence: high.
- Escalation: none.
<!-- agent:boundary:f01e7e2e -->
<!-- /agent:exchange -->

## Queue

<!-- agent:queue preset="#spec-test-build-install-commit-push" go -->
<!-- /agent:queue -->
<!--
https://github.com/chopratejas/headroom
-->

## Backlog

<!-- agent:backlog priority queue -->
- [ ] [#hccr2] [recommended] Implement reversible retrieval handles for truncated tsift search/source/context-pack/digest output, storing omitted originals in .tsift/cache with query-scoped retrieve commands.
- [ ] [#hsmart2] [recommended] Add adaptive sampling for large JSON/list outputs in tsift envelopes and digests, preserving errors, first/last rows, anomalies, and query-relevant rows before budget truncation.
- [ ] [#hcache2] [recommended] Add stable-prefix and prompt-cache layout checks to session-review/context-pack output, separating stable instructions/evidence from volatile dates, paths, watermarks, and run ids.
- [ ] [#hlearn2] [recommended] Add failure-learning mining over session-review/log-digest/test-digest history, producing marker-owned AGENTS or tsift-memory corrections for repeated command/path/search failures.
- [ ] [#hmcp2] [recommended] Add first-party tsift MCP tools for compress/retrieve/stats-style workflows, reusing existing envelope, digest, and token-savings metrics rather than requiring a proxy.
<!-- /agent:backlog -->

## Review

<!-- agent:review -->
<!-- /agent:review -->

<!-- agent:icebox -->
- [ ] [#g5kw] What are #next-steps to promote SurrealDB to SQLite-equivalent substrate status?
- [ ] [#4z0w] Can we increase max-hop caps?
- [ ] [#cg52] Could FalkorDB help with performance?
<!-- /agent:icebox -->

## Completed / Reaped

<!-- agent:done archive=tasks/software/tsift.done.md -->

<!-- completed work archived in tasks/software/tsift.done.md -->
<!-- /agent:done -->
