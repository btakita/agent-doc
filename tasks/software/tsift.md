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

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc65443c45f7b49f5e6ace54e3f6bafe4077ade57924422b5eb3baf4dbd23c-20260610-160745.md`*

Compacted content:
- Archived 2 response topic(s): agent-doc coupling; x5fw
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc6544...
- Escalation: none.
- Confidence: high on feature fit and priority; medium on implementation sizing because each item needs a focused design slice.
- Verification: research-only turn; no code tests run. Closeout verification is the agent-doc finalize/session-check path.
- Commands: `tsift status`, `tsift status --fix`, `tsift index --workspace .`, `agent-doc preflight`, `agent-doc plan`, `agent-doc read`, tsift source/search/symbol reads for tsift spec/README/output/digest/context-pack/search-budget/token-savings surfaces, and `git status --short` checks.
- Changed paths: session document/backlog only via agent-doc finalize; no `src/tsift` code/spec edits in this cycle.
- ML-heavy text compression should stay out of the default path. tsift's value is predictable, inspectable output; use learned ranking and retrieval before opaque semantic compression.
- Headroom's AST code-body compression is only partly relevant. tsift should not collapse source bodies by default because exact source is often the point; it could add an opt-in `source-read --compressed-body` mode later, but reversible handles and better budget selection matter more.
- Headroom's agent proxy/wrapper is less relevant. tsift is strongest as a deterministic local evidence tool, not as a provider-intercepting proxy.
5. **MCP-style compress/retrieve/stats surface.** Headroom exposes compression, retrieval, and stats as MCP tools. tsift should expose `tsift_search`, `tsift_source_read`, `tsift_context_pack`, `tsift_retrieve`, and `tsift_stats` as a first-party MCP server so Claude/Codex/OpenCode can use tsift's native envelopes without shell rewrites. This is especially attractive because tsift already has token-savings fixtures and structured envelopes; MCP would make those surfaces easier for hosts to call correctly.
4. **Failure-learning loop.** Headroom's `learn` mines failed sessions for command/path/search corrections and writes marker-owned project learnings. tsift has better raw ingredients for this than Headroom: `session-review`, `log-digest`, `test-digest`, memory events, and graph-backed source handles. A tsift version should identify repeated failed commands, wrong path guesses, overly broad searches, stale-index misses, and recovery commands, then propose marker-owned AGENTS.md / `.tsift/memory.db` corrections.
3. **CacheAligner-style stable-prefix layout.** Headroom separates volatile prompt material from stable prefix material to improve provider cache hits. tsift can apply the same idea without touching providers: `session-review`, `context-pack`, and agent-doc packets should emit stable sections first and volatile watermarks, timestamps, run ids, and current-date facts later. tsift already reports prompt-cache ROI in session-cost/session-review; the next step is a lint/gate that detects volatile fields too early in an agent packet.
2. **SmartCrusher-style adaptive sampling for structured output.** Headroom's JSON/list compressor preserves first/last items, errors, anomalies, and query-relevant rows before sampling the rest. tsift currently caps previews deterministically by `max_items` / `max_bytes`; for large JSON, test logs, search hits, graph rows, and context-pack subsections, it should rank retained rows by error signal, recency, file diversity, graph centrality, and query relevance before budget truncation. This would make small/normal/deep budgets safer under high-hit search and noisy digest cases.
1. **CCR-style reversible envelope compression.** Headroom's Compress-Cache-Retrieve model caches the original locally and gives the model a retrieve handle when compressed content is insufficient. tsift should generalize its existing expansion commands into a true retrieval contract: every truncated search result set, digest section, context-pack section, and source window can carry a stable cache handle plus a `tsift retrieve <handle> [--query ...]` or existing narrow expansion command. This is the highest-value fit because tsift already emits handles and resume commands; the missing piece is a uniform local original store and query-within-original path.
- Escalation: none.
- Verification: this is a queue-state repair for an accidental recommendation promotion; no code tests run.
- Commands: `agent-doc reset --from-current`, `agent-doc preflight`, `agent-doc admin inspect`, `agent-doc admin queue pause`, `agent-doc session-check` attempt, and queue/backlog behavior inspection.
- Changed paths: session document state only; no `src/tsift` implementation/spec edits.
<!-- agent:boundary:ae634441 -->
<!-- /agent:exchange -->

## Queue

<!-- agent:queue preset="#spec-test-build-install-commit-push" -->
<!-- /agent:queue -->
<!--
https://github.com/chopratejas/headroom
-->

## Backlog

<!-- agent:backlog priority queue -->
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
