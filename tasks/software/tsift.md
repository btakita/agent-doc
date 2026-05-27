---
agent_doc_session: tsift-v0.1
agent: opencode
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
queue_active: true
---

## Status

<!-- agent:status patch=replace -->

OpenCode npm package path code-complete and pushed. #gopencode-install and #gopencode gated on npm registry auth to publish opencode-tsift. 5 of 7 backlog items completed: integration tests, permission docs, transcript verify, smoke session, README expansion.
<!-- /agent:status -->

## Spec

See [src/tsift/SPEC.md](../../src/tsift/SPEC.md) for the AST-Aware Code RAG spec.

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc65443c45f7b49f5e6ace54e3f6bafe4077ade57924422b5eb3baf4dbd23c-20260527-234100.md`*

Compacted content:
- Archived 3 response topic(s): local-e2e-verify; tsift + tokensave integration; libsql backend implementation
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc6544...
❯ Can algorithms and pipelines from tokensave be used in tsift? Would it be beneficial?
### Re: tokensave algorithms in tsift — glm-5.1

**Yes, selectively — but not wholesale.**

#### High-value integration paths

| tokensave capability | tsift benefit | Integration approach |
|---|---|---|
| **34-language tree-sitter extraction** | tsift covers ~10 languages; tokensave covers 34 | Delegate extraction to tokensave for uncovered languages via GraphStore adapter |
| **Code-health analytics** (health scores, god class, dead code, circular deps, coupling, DSM, Lakos levelization) | tsift has no equivalent; high-value for conflict-matrix and dispatch-trace | Wrap tokensave tools inside tsift digest pipeline with budget-aware envelopes |
| **Call-graph impact analysis** (`impact`, `affected`, `hotspots`) | tsift has BFS path/neighborhood but lacks topological impact scoring | Pipe into tsift `conflict-matrix` gate for smarter parallel dispatch |
| **Shared libSQL storage** | `LibsqlGraphStore` already uses same libSQL backend | Shared `.db` schema could reduce re-indexing between both tools |

#### What NOT to absorb

- **48 MCP tools + edit primitives** — months of work for marginal gain; tsift delegates edits to the harness
- **Multi-branch indexing** — tsift projection/versioning model is architecturally different
- **Cost tracking + TUI monitor** — no tsift equivalent needed; tokensave owns this
- **FTS5 semantic search** — tsift uses graph queries, not full-text search

#### Concrete next step

1. Implement a `tokensave` backend adapter in `substrate.rs` behind `backend-eval` feature flag
2. Read tokensave `.tokensave/tokensave.db` directly via `LibsqlGraphStore::open()`
3. Benchmark through `perf_gate.rs` against SQLite

This lets tsift query tokensave pre-built graph without re-indexing — highest-ROI integration with minimal new code.
What are #next-steps to reverse-engineer tokensave to directly implement in tsift?
<!-- agent:boundary:d34bc168 -->
<!-- /agent:exchange -->
###
<!--
What are #next-steps to make the graph faster?
---
What are #next-steps to make the graph + neighborhoods faster + more effective?
-->

<!-- agent:queue auto -->
preset #spec-test-build-install-commit-push
- ~[#local-e2e-verify]~
- ~[#npm-auth-setup]~
- ~[#npm-publish-verify]~
- ~[#full-e2e-verify]~
- What are #next-steps to reverse-engineer tokensave to directly implement in tsift?
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog -->
- [ ] [#npm-auth-setup] Configure NPM_TOKEN secret + TSIFT_ENABLE_NPM_PUBLISH=true variable in GitHub repo settings
- [ ] [#npm-publish-verify] After auth configured, tag release and verify publish-opencode-plugin CI job succeeds and opencode-tsift appears on npmjs.org
- [ ] [#full-e2e-verify] In fresh project: opencode plugin opencode-tsift && opencode then /tsift-status. On success mark #e2e-install done.
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
<!-- /agent:icebox -->

## Completed / Reaped

<!-- agent:done archive=tasks/software/tsift.done.md -->

<!-- completed work archived in tasks/software/tsift.done.md -->
<!-- /agent:done -->
