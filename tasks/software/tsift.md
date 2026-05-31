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

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc65443c45f7b49f5e6ace54e3f6bafe4077ade57924422b5eb3baf4dbd23c-20260531-191524.md`*

Compacted content:
- Archived 1 response topic(s): #ranked-neighborhood
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc6544...

### Re: #terse-health-early-exit — glm-5.1

> **Queue prompt:**
>
> [#terse-health-early-exit]


Rewrote terse_health_report. All 15 tests pass. Committed as 5cf8093, pushed to main.

### Re: #libsql-edges-between-pushdown — glm-5.1

> **Queue prompt:**
>
> [#libsql-edges-between-pushdown]


Overrode `edges_between_nodes` in `LibsqlGraphStore` with temp-table + EXISTS pushdown, matching the SQLite backend pattern (`tsift-sqlite/src/lib.rs:2235`).

**Changes:**
- `tsift-libsql/src/lib.rs` — added `edges_between_nodes` override using `CREATE TEMP TABLE _edges_between_ids`, batch inserts (chunks of 450), and `EXISTS` subqueries against `graph_edges`
- `tsift-libsql/src/lib.rs` — added `libsql_store_edges_between_nodes_pushdown` test verifying scoped filtering, empty set, and single-node edge cases
- `SPEC.md` — updated pushdown queries section to document the new `edges_between_nodes` override

**Verification:** 8/8 tests pass. Built and pushed as `4693e36`.

### Re: #compact-community-serialization — glm-5.1

> **Queue prompt:**
>
> [#compact-community-serialization]


Added `TerseCommunityMember`, `TerseCommunity`, and `TerseCommunityResult` types to `tsift-graph/src/lib.rs` for compact community serialization.

**Types:**
- `TerseCommunityMember` — `name` + `tagpath_handle` only (strips `file`, `line`, `refs`)
- `TerseCommunity` — wraps top-N `TerseCommunityMember` slice + `id` + `modularity_contribution`
- `TerseCommunityResult` — terse result with same aggregate fields as `CommunityResult`
- `CommunityResult::to_terse(top_n)` — converts full result to truncated terse form

**Tests added:** 4 new tests — field stripping, top-N truncation, `to_terse` from `detect_communities`, member count reduction.

**Spec updated:** Community Detection section + tsift-graph crate listing.

**Verification:** 71/71 tests pass. Built and pushed as `26762b0`.

### Re: #louvain-dedup-aggregate — glm-5.1

> **Queue prompt:**
>
> [#louvain-dedup-aggregate]


Added Louvain phase-2 super-node coarsening to `detect_communities` in `tsift-graph/src/lib.rs`.

**Algorithm changes:**
- Extracted `LouvainGraph` struct with `phase1()` (returns community assignment) and `coarsen()` (collapses communities into weighted super-nodes with self-loops)
- Phase 2 loop: coarsen graph → run phase 1 on super-nodes → map back to original nodes → repeat up to 10 levels until no improvement
- Self-loops in coarsened graph carry half the internal edge weight, matching standard Louvain degree accounting

**Spec updated:** Algorithm section now describes the multi-level process (phase 1 local moving, phase 2 coarsening, repeat).

**Test added:** `communities_hierarchical_phase2_improves_modularity` — 4 clusters of 6 nodes each with bridging edges, verifies ≥ 2 communities and positive modularity.

**Verification:** 72/72 tests pass. Built and pushed as `db4d690`.
<!-- agent:boundary:0c9627bd -->
<!-- /agent:exchange -->

## Queue

<!-- agent:queue auto -->
preset #spec-test-build-install-commit-push
- ~[#libsql-pushdown-edge-incident]~
- ~[#health-dedup-scc]~
- ~[#ranked-neighborhood]~
- ~[#terse-health-early-exit]~
- ~[#libsql-edges-between-pushdown]~
- ~[#compact-community-serialization]~
- ~[#louvain-dedup-aggregate]~
- [#shared-graph-build-pipeline]
- [#sift-token-index]
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog -->
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
