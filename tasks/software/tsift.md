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
queue_active: false
---

## Status

<!-- agent:status patch=replace -->
<!-- /agent:status -->

## Spec

See [src/tsift/SPEC.md](../../src/tsift/SPEC.md) for the AST-Aware Code RAG spec.

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc65443c45f7b49f5e6ace54e3f6bafe4077ade57924422b5eb3baf4dbd23c-20260530-184920.md`*

Compacted content:
- Archived 2 response topic(s): monorepo and multiplicity next steps; queued multiplicity implementation
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc6544...
❯ Read file:///home/brian/.claude/usage-data/report-2026-05-30-112308.html. What are #next-steps for tsift to save tokens and improve effectiveness of graph/neighborhood queries?

### Re: token savings, graph neighborhoods, and claude-mem — gpt-5

I read the Claude Code Insights report and inspected the local `claude-mem` installation, database, logs, and HTTP API. The report is pointing at a real tsift opportunity: not generic “more memory,” but budgeted memory and graph retrieval that prevents large observer prompts from ever being constructed.

Evidence:
- Report scope: 3,888 messages across 317 sessions from May 28-30, with observer/memory sessions called out as a major part of the workflow and a major source of failure.
- Report friction: `claude-mem` observer sessions repeatedly hit `Prompt is too long`, sometimes producing zero usable memory output.
- Local `claude-mem`: plugin `10.5.2` is running as a Bun worker with Chroma MCP; settings currently keep `CLAUDE_MEM_CONTEXT_OBSERVATIONS=50`, `CLAUDE_MEM_CONTEXT_SESSION_COUNT=10`, and terminal output enabled.
- Local data: `claude-mem.db` contains 32,337 observations and 7,400 summaries, so the data is valuable. But `pending_messages` has 6,434 failed rows, 74 pending rows, and 1 processing row.
- Local logs: May 28-30 logs include 223 context-overflow errors and 228 `Prompt is too long` lines.
- tsift’s own `session-review --next-context` for this document flags the same class of waste: largest prompt turn 477,500 tokens, cached input ratio 98.58% across roughly 548M prompt tokens, restart churn, and repeated no-op closeouts.
- The `claude-mem` HTTP API works, but its retrieval is too shallow for this failure class: searching observations for `prompt too long` returned unrelated agent-doc observations instead of the observer overflow failures.

Recommendation: do not hard-drop `claude-mem` today. Keep it as a capture/UI/history source because it has a large existing corpus and a working local API. But tsift should own the durable memory contract: budgeted handoff construction, graph projection, ranked retrieval, evidence packets, and persistence verification. Treat `claude-mem` as a provider/importer until it proves reliable under explicit token budgets.

I added the follow-up backlog in priority order. Top item: `tsift-memory-budget-guard`.

Priority order:
1. `tsift-memory-budget-guard` — prevent observer/memory prompts from exceeding budget before model calls.
2. `tsift-claude-mem-adapter` — import existing claude-mem memory into tsift graph reads without taking over capture immediately.
3. `tsift-neighborhood-token-budget` — make `graph-db related` / `neighborhood` / `evidence` optimize for useful tokens, not just node count and depth.
4. `tsift-memory-eval-harness` — compare claude-mem and tsift retrieval on this actual failure class.
5. `tsift-summary-extract-gate` — make missing summaries an explicit semantic-graph readiness gate.

Changed paths: session document/backlog only; no source edits.

Commands used: `tsift status`, `tsift status --fix`, `agent-doc preflight`, `agent-doc orchestrate --from-exchange --plan`, `agent-doc plan`, `agent-doc read`, `lynx -dump` on the report, targeted `sqlite3` queries against `claude-mem.db`, local `curl` calls to the `claude-mem` API, targeted `tsift` searches/source reads, `tsift --envelope session-review --next-context --budget normal`, and `tsift --envelope context-pack --budget normal`.

Verification: document-analysis cycle only; source verification was not required because no code changed. The key checks were the usage-report read, local database/log/API inspection, and tsift’s own next-context/context-pack output.

Confidence: high on the direction. The only caveat is that the current active `claude-mem` worker is a minified plugin cache, so implementation should target either its stable HTTP/SQLite surfaces or a tsift-owned adapter rather than patching cached plugin internals directly.
Please analyze claude-mem. Should we drop it or have our own?
<!-- agent:boundary:e2c920df -->
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
- [ ] [#tsift-memory-budget-guard] Build a token-budget guard for observer/memory handoffs: estimate prompt size before model calls, split oversized tool responses and transcripts, replace raw logs with tsift session-digest/context-pack handles, and fail closed with retryable chunk plans instead of sending prompts that can only overflow.
- [ ] [#tsift-claude-mem-adapter] Add a read-only claude-mem adapter for tsift graph-db: project claude-mem SQLite observations, session summaries, user prompts, and Chroma/vector handles as semantic/session/source nodes with stable ids so tsift can query existing memory without owning capture yet.
- [ ] [#tsift-neighborhood-token-budget] Add token/usefulness budgets to graph-db related/neighborhood/evidence output: rank nodes and edges by semantic match, edge kind, depth, recency, and source-handle coverage; enforce per-kind quotas plus estimated-token caps; report dropped_by_budget diagnostics.
- [ ] [#tsift-memory-eval-harness] Create a memory retrieval eval fixture from the Claude Code Insights report plus sampled claude-mem failures, comparing claude-mem API, tsift session-review/context-pack, and graph-db related on useful hits, output tokens, latency, and zero-output failure rate.
- [ ] [#tsift-summary-extract-gate] Promote missing summary cache from reminder to graph effectiveness gate: context-pack/graph-db related should provide the exact summarize --extract and graph refresh commands required before semantic neighborhoods are trusted.
<!-- /agent:backlog -->

## Review

<!-- agent:review -->
- [/] [#full-e2e-verify] In fresh project: opencode plugin opencode-tsift && opencode then /tsift-status. On success mark #e2e-install done.
- [/] [#npm-publish-verify] After auth configured, tag release and verify publish-opencode-plugin CI job succeeds and opencode-tsift appears on npmjs.org
- [/] [#npm-auth-setup] Configure NPM_TOKEN secret + TSIFT_ENABLE_NPM_PUBLISH=true variable in GitHub repo settings
- [/] [#e2e-install] Verify end-to-end install: fresh opencode project + opencode plugin opencode-tsift + /tsift-status
<!-- /agent:review -->
###
<!--
Can we increase max-hop caps?
Could FalkorDB help with performance?
-->

<!-- agent:icebox -->
- [ ] [#npm-auth-setup] Configure NPM_TOKEN secret + TSIFT_ENABLE_NPM_PUBLISH=true variable in GitHub repo settings
- [ ] [#npm-publish-verify] After auth configured, tag release and verify publish-opencode-plugin CI job succeeds and opencode-tsift appears on npmjs.org
- [ ] [#full-e2e-verify] In fresh project: opencode plugin opencode-tsift && opencode then /tsift-status. On success mark #e2e-install done.
<!-- /agent:icebox -->

## Completed / Reaped

<!-- agent:done archive=tasks/software/tsift.done.md -->

<!-- completed work archived in tasks/software/tsift.done.md -->
<!-- /agent:done -->
