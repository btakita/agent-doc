---
agent_doc_session: tsift-v0.1
agent: claude
claude_model: opus
agent_doc_format: template
agent_doc_write: crdt
claude_args: --dangerously-skip-permissions
codex_args: -s danger-full-access
prompt_presets:
  '#advance-review': Go through the review items. Add actionable backlog items to complete or advance the review items.,
  '#agent-doc-bug': Please create a plan for agent-doc to fix this issue. Add to the backlog of agent-loop/tasks/agent-doc/agent-doc-bugs2.md
  '#bugs-observed': Please add any bugs from observed behavior to the backlog.,
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

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc65443c45f7b49f5e6ace54e3f6bafe4077ade57924422b5eb3baf4dbd23c-20260613-083357.md`*

Compacted content:
- Archived 4 response topic(s): do [#tsreview2]; do [#gdblockcover]; do [#pcacheexplattr]; 1 more
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc6544...

### Re: #bugs-observed — capture observed tsift bugs to backlog — opus-4-8

One reproducible tsift bug observed this session, added to the backlog as **#cargoidxcov**.

**#cargoidxcov — partial cargo-index coverage not surfaced by `status`.** While reading `packages/tsift-agent-doc/src/session_cost.rs` with `tsift source-read`, every call warned `index refs unavailable: no index found at .../indexes/cargo/tsift-agent-doc/index.db` and degraded to window-only output (no AST symbol refs / child refs / graph expansions). Investigation: the workspace has 23 members but `.tsift/indexes/cargo/` holds only 9 (`tsift`, `tsift-cli`, `tsift-core`, `tsift-graph`, `tsift-index`, `tsift-memory`, `tsift-quality`, `tsift-sqlite`, `tsift-status`). The other 14 — including `tsift-agent-doc` — have no per-crate cargo index, so `source-read`/`symbol-read` silently lose symbol projection for ~60% of packages. Yet `tsift status` reports the index `fresh (188 files)` and recommends `use: search, explain, graph` with no signal about the gap. (`tsift search` still resolves symbols via the 20.6M main `index.db`, so search isn't affected — only the per-crate symbol-ref path.) The fix cycle can decide between building cargo indexes for all members on refresh vs. having `status` report partial coverage + a targeted reindex hint.

Out of scope for this doc but noted: the **agent-doc IPC ack-timeout / exit-75-on-finalize** behavior I hit this session is an agent-doc bug, not a tsift one — it belongs in `tasks/agent-doc/agent-doc-bugs2.md` (per the `#agent-doc-bug` preset), so I did not add it to tsift's backlog.

Backlog top: **#cargoidxcov**.
<!-- agent:boundary:30cc805f -->
<!-- /agent:exchange -->

## Queue

<!-- agent:queue preset="#spec-test-build-install-commit-push" go -->
- do [#cargoidxcov]
<!-- /agent:queue -->
<!--
https://github.com/chopratejas/headroom
-->

## Backlog

<!-- agent:backlog priority queue -->
- [ ] [#cargoidxcov] tsift index coverage gap: status reports the index fresh (188 files) and recommends use: search/explain/graph, but only 9 of 23 workspace members have a .tsift/indexes/cargo/<pkg>/index.db. packages/tsift-agent-doc plus 13 other members (tsift-libsql, tsift-tokensave, tsift-memgraphrag, tsift-md-ast, tsift-algorithms, tsift-resolution, tsift-cache, tsift-session, tsift-summarize, tsift-digest, tsift-search, tsift-surrealdb, tsift-sim-world) lack one, so source-read/symbol-read for files in those packages emit "index refs unavailable: no index found at .../indexes/cargo/<pkg>/index.db" and silently fall back to window-only output with no AST symbol refs. status does not surface this partial cargo-index coverage, so an agent trusting fresh gets degraded symbol projection for ~60% of packages and no actionable fix hint. Fix: build cargo indexes for all workspace members on index/refresh, or have status report partial cargo coverage and emit a targeted reindex/--fix hint for uncovered members. Observed 2026-06-13.
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
