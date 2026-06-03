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

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc65443c45f7b49f5e6ace54e3f6bafe4077ade57924422b5eb3baf4dbd23c-20260603-041745.md`*

Compacted content:
- Archived 1 response topic(s): reduce Code Navigation token usage
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc6544...

### Re: #gfrontierhot Performance Triage — glm-5.1

Three-sample SQLite performance triage on the real agent-loop graph (11,181 nodes, 30,989 edges):

| Operation | Sample 1 | Sample 2 | Sample 3 | Median |
|-----------|----------|----------|----------|--------|
| Neighborhood (out-degree 3,227, depth 3) | 126ms | 112ms | 111ms | **112ms** |
| Neighborhood (in-degree 4,602, depth 3) | 40ms | 39ms | 39ms | 39ms |
| Path (between hub nodes) | 7ms | 6ms | 6ms | 6ms |
| Evidence (depth 3) | 7ms | 7ms | 7ms | 7ms |
| Related (semantic search) | 14ms | 5ms | 4ms | 5ms |
| Deep-chain path | 6ms | 5ms | 8ms | 6ms |

**Hotspot:** Neighborhood on high-out-degree file nodes at depth 3 (~112ms). All other operations are <15ms. The bottleneck is recursive CTE frontier expansion on file→symbol `defines` edges (3,227 outgoing per file node). The shared CTE approach works well for inbound-heavy nodes (39ms). No optimization warranted for path, evidence, or related at current graph sizes.

### Re: #greadiness Actionable Readiness at Lookup Boundary — glm-5.1

Three code changes in `packages/tsift-cli/src/lib.rs`:

1. **Context-pack evidence suppression** (`context_pack_graph_orchestration`): When semantic readiness is blocked, skips evidence target resolution and conflict-matrix expansion entirely. Emits repair decision in `conflict_matrix_decisions` instead.

2. **Evidence semantic readiness warning** (`graph_db_evidence_report_from_store`): Adds semantic readiness check after freshness gate. When blocked, emits warning with exact repair commands (index → summarize → refresh chain) instead of proceeding silently with low-signal traversal.

3. **Complete repair chain** (`graph_db_semantic_readiness`): For `summary_cache_empty`, now includes index command before summarize when stale. For `summary_cache_unavailable`, adds summarize + refresh after index build. Every blocked state now emits a complete, ordered repair chain.

Updated acceptance fixture `agent-orchestration-acceptance-pack.json` for new context-pack behavior. All 1,135 workspace tests pass.

### Re: #rankdefault Ranked Neighborhood Comparison — glm-5.1

Added `ranked_neighborhood_comparison` diagnostic field to the neighborhood CLI output. Runs `GraphStore::ranked_neighborhood` (EdgeKindWeighted scoring) alongside the existing unranked BFS and compares:

- Useful-hit density (ranked: 0.504 vs unranked: 0.562)
- Duplicate name count (ranked: 1 vs unranked: 0)
- Handle coverage (both 100%)
- Overlap (0.5% — approaches differ in budget strategy)
- Latency (ranked: 1,272ms vs unranked BFS: 112ms)

The ranked traversal pruned 3,027 of 3,228 discovered nodes (94%), demonstrating aggressive budget control. However, it's ~11x slower due to per-frontier-node `outgoing_edges()` + `node()` calls vs the batched CTE approach. The comparison is diagnostic-only — promotion requires the community-search quality gate to pass. Gate remains `held_default_order_unchanged`.

### Re: Module Extraction from tsift-cli/lib.rs — glm-5.1

Extracted two modules from `packages/tsift-cli/src/lib.rs` (44,152 → 43,447 lines, -705 net):

**New modules:**
- `workflow.rs` (~150 lines): `WorkflowRecipe`, `WorkflowStep`, `search_workflow_recipe`, `cmd_workflow`
- `token_savings.rs` (~570 lines): `TokenSavingsReport`, `TokenSavingsFixture`, all envelope/report types, `cmd_token_savings`

**Pattern established:**
1. Move structs + functions to new module file
2. Make items `pub(crate)` for cross-module access
3. Add `mod new_module;` to lib.rs
4. Re-export types via `#[cfg(test)] use new_module::{...}` for test compatibility
5. Update call sites in `run()` to `module::fn_name()`

**Verification:** All 1,135 workspace tests pass. `tsift --envelope workflow search --json` confirmed working post-install.

**Remaining phases (~41,700 lines):**
- Phase 2: Rewrite (~1,400 lines), Community Detection (~450), Dependency DAG (~700), Dispatch Trace (~350)
- Phase 3: Semantic Edit (~4,000), Traversal/Exploration (~1,300)
- Phase 4: Convex (~3,000), Graph DB core (~5,000+)
- Phase 5: Context Pack (~1,700), Conflict Matrix (~2,500), Search Budget (~800), Session Review (~500)

Queue items `do [#rankdefault]`, `do [#gfrontierhot]`, `do [#greadiness]` were auto-struck as already resolved. Queue items for OpenCode/Codex tsift integration are captured in backlog.

### Re: #next-steps OpenCode/Codex tsift Integration — glm-5.1

Backlog items added for:
- OpenCode: replace Read tool with tsift AST API for source reading
- OpenCode: replace grep with tsift search for content search
- Codex: adopt tsift AST API for source reading
- Remaining lib.rs modularization phases
<!-- agent:boundary:16501a77 -->
<!-- /agent:exchange -->
###
<!--
What are #next-steps to promote SurrealDB to SQLite-equivalent substrate status?
-->

## Queue

<!-- agent:queue preset="#spec-test-build-install-commit-push" -->
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog queue -->
- [ ] [#libmod3] Phase 3: Extract Semantic Edit (~4,000 lines), Traversal/Exploration (~1,300) from lib.rs
- [ ] [#libmod2] Phase 2: Extract Rewrite (~1,400 lines), Community Detection (~450), Dependency DAG (~700), Dispatch Trace (~350) from lib.rs
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
