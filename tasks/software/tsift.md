---
agent_doc_session: tsift-v0.1
resume: 019e85d7-f9ee-79f3-b597-4ed352495bbd
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
<!-- /agent:status -->

## Spec

See [src/tsift/SPEC.md](../../src/tsift/SPEC.md) for the AST-Aware Code RAG spec.

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc65443c45f7b49f5e6ace54e3f6bafe4077ade57924422b5eb3baf4dbd23c-20260603-160632.md`*

Compacted content:
- Archived 1 response topic(s): #libmod3 Phase 3
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc6544...

### Re: #libmod4 Phase 4: Extract Convex + GraphStore into tsift-core sub-modules — glm-5.1

> **Queue prompt:**
>
> do [#libmod4]


Completed. Split `tsift-core/src/lib.rs` (1999 lines monolith) into three focused sub-modules:

**New files:**
- `packages/tsift-core/src/types.rs` — All graph data types (GraphNode, GraphEdge, GraphProjection, etc.) + id helpers (`stable_graph_edge_id`, `graph_edge_id`)
- `packages/tsift-core/src/store.rs` — GraphStore trait with full default implementations + query/paging helpers + `shortest_path_using_outgoing`
- `packages/tsift-core/src/convex.rs` — Convex adapter (ConvexProjectionRows, ConvexNodeRow, ConvexEdgeRow, ConvexGraphClient trait, ConvexRowsGraphClient, ConvexGraphStore)

**Updated:**
- `packages/tsift-core/src/lib.rs` — Re-exports all public types at crate root via `pub use` for backward compatibility; retains all 15 tests
- `SPEC.md` — Architecture section updated to show three sub-modules under tsift-core
- Also included prior Phase 3 CLI extractions (`semantic_edit.rs` from cli/lib.rs, unused import cleanup)

**Verification:**
- All 637 workspace tests pass (`make check` green)
- All 8 downstream crates compile cleanly with zero source changes
- `cargo install --path .` succeeds
- Pushed as `b4fc070` on main
<!-- agent:boundary:205b225b -->
<!-- /agent:exchange -->
###
<!--
What are #next-steps to promote SurrealDB to SQLite-equivalent substrate status?
-->

## Queue

<!-- agent:queue preset="#spec-test-build-install-commit-push" auto -->
- ~do [#libmod3]~
- ~do [#libmod4]~
- do [#libmod5]
- do [#opencode-read]
- do [#opencode-grep]
- do [#codex-read]
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog -->
- [ ] [#surreal-warm] Warm-start SurrealDB store: reuse SurrealdbGraphStore across backend-eval runs instead of recreating; measure real-workload median with persistent store
- [ ] [#surreal-delta] Delta projection refresh: implement replace_projection_rows_delta() with row-hash diffing matching SQLite's replace_projection_with_version() pattern
- [ ] [#surreal-multiproc] Multi-process lock behavior: write conformance test with N concurrent reader processes + 1 writer against same SurrealKV path; document lock contention, stale-read behavior, and crash recovery (SPEC.md:304 promotion gate)
- [ ] [#surreal-rt-pool] Shared tokio runtime: accept external Runtime reference or use lazy static instead of per-store Runtime::new()
- [ ] [#surrealbulk-close] Close #surrealbulk review item: re-run benchmarks, update SPEC.md benchmark table, mark done
- [ ] [#libmod5] Phase 5: Extract Context Pack (~1,700), Conflict Matrix (~2,500), Search Budget (~800), Session Review (~500) from lib.rs
- [ ] [#opencode-read] OpenCode: replace Read tool with tsift AST API (tsift --envelope source-read) for source code reading
- [ ] [#opencode-grep] OpenCode: replace grep/rg with tsift search for content search
- [ ] [#codex-read] Codex: adopt tsift AST API for source reading instead of raw file reads
<!-- /agent:backlog -->

## Review

<!-- agent:review -->
- [/] [#rankdefault] Promote ranked_neighborhood into default graph lookup/context-pack/evidence output only after quality gates pass; compare ranked vs unranked output on real backlog/code queries for useful-hit density, duplicate-name precision, handle coverage, latency, and emitted tokens.
- [/] [#surrealbulk] Batch SurrealDB projection load: replace per-row upsert loops with bulk/transactional row load plus one-pass secondary-index rebuild; measure refresh/load separately.
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
