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
<!-- /agent:status -->

## Spec

See [src/tsift/SPEC.md](../../src/tsift/SPEC.md) for the AST-Aware Code RAG spec.

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc65443c45f7b49f5e6ace54e3f6bafe4077ade57924422b5eb3baf4dbd23c-20260531-163348.md`*

Compacted content:
- Archived 6 response topic(s): queue; queue re-activation; fix preexisting warnings; 3 more
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc6544...

### Re: #libsql-pushdown-edge-incident — glm-5.1

> **Queue prompt:**
>
> [#libsql-pushdown-edge-incident]


Added `incident_edges()` override to `LibsqlGraphStore` with a UNION of two indexed probes on `from_id` (via `idx_graph_edges_from_kind`) and `to_id` (via `idx_graph_edges_to_kind`), matching the SQLite backend's `sqlite_incident_edges_union_query` pattern. The `edge()` method was already overridden with an indexed `edge_key` lookup, so only `incident_edges()` needed the pushdown.

**Changes:**
- `packages/tsift-libsql/src/lib.rs` — added `incident_edges()` impl with UNION query + `libsql_incident_edges_pushdown` test (7/7 tests pass)
- `SPEC.md` — added "Pushdown queries" paragraph documenting both `edge()` and `incident_edges()` indexed overrides

**Verification:** `cargo test -p tsift-libsql` (7 pass), `cargo build --release`, `cargo install --path .`, committed as `22bbd76` and pushed.

### Re: test

Hello world
<!-- agent:boundary:ba9a8b5b -->
<!-- /agent:exchange -->

## Queue

<!-- agent:queue auto -->
preset #spec-test-build-install-commit-push
- ~[#libsql-pushdown-edge-incident]~
- [#health-dedup-scc]
- [#ranked-neighborhood]
- [#terse-health-early-exit]
- [#libsql-edges-between-pushdown]
- [#compact-community-serialization]
- [#louvain-dedup-aggregate]
- [#shared-graph-build-pipeline]
- [#sift-token-index]
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog -->
- [ ] [#ranked-neighborhood] Add ranked neighborhood with configurable scoring (BreadthFirst, EdgeKindWeighted, DegreeWeighted) and max_nodes budget. ~30-60% token reduction for explain/neighborhood. tsift-core/lib.rs:598-668
- [ ] [#terse-health-early-exit] Early-terminated terse health report: stop after top/bottom N scores, skip per-node sub-metrics when only overall is needed. ~30% health compute reduction. tsift-algorithms/src/health.rs:42-71
- [ ] [#libsql-edges-between-pushdown] Override edges_between_nodes in LibsqlGraphStore with temp-table + EXISTS (parity with SQLite backend). 5-20x neighborhood speedup. tsift-libsql/src/lib.rs
- [ ] [#compact-community-serialization] Add TerseCommunityMember/TerseCommunity — name + tagpath_handle only, top-N member slice. ~40-60% community JSON savings. tsift-graph/src/lib.rs:541-572
- [ ] [#louvain-dedup-aggregate] Add Louvain phase-2 aggregation (super-node coarsening). 5-15% modularity improvement on hierarchical codebases. tsift-graph/src/lib.rs:590-743
- [ ] [#shared-graph-build-pipeline] Extract shared build_graph() from health.rs to common module; reuse in scc.rs, coupling.rs, dead_code.rs. ~30% analyze wall-time reduction. tsift-algorithms/src/*.rs
- [ ] [#sift-token-index] Build inverted token index for search pre-filtering — skip files with no matching tokens. 5-50x search speedup on large codebases. tsift-search/src/sift.rs:210-267
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
