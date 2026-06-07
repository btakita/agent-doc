---
agent_doc_session: tsift-v0.1
resume: 019e85d7-f9ee-79f3-b597-4ed352495bbd
agent: claude
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
<!-- /agent:status -->

## Spec

See [src/tsift/SPEC.md](../../src/tsift/SPEC.md) for the AST-Aware Code RAG spec.

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc65443c45f7b49f5e6ace54e3f6bafe4077ade57924422b5eb3baf4dbd23c-20260607-025440.md`*

Compacted content:
- Archived 4 response topic(s): Run the gated backlog items; Commit all uncommitted tsift changes; Ontology / Fact / Passage Graph mapping onto tsift; 1 more
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc6544...

### Re: do #trt1 + #memgraphrag-ont — opus-4-8

> **Queue prompt:**
>
> do [#memgraphrag-ont]


Both implemented (core), tested, built/installed, committed + pushed (`btakita/tsift` `main` `b300196`). Marked `#trt1` and `#memgraphrag-ont` done. You queued `#trt1` ahead of `#memgraphrag-ont` — correct order, since the ontology derives empirically over whatever node kinds exist, so authored findings types now surface in it automatically.

**`#trt1` — authored findings nodes (core)** (`tsift-memory` + `tsift-cli`)
- `AuthoredNodeKind` (`finding`/`decision`/`note`) + `authored_node_projection(...)`: a content-stable node anchored to a **stable symbol handle** (graph id / tagpath — not a line number) via an `annotates` edge, with `confidence` (clamped 0..=1), `observed_at_unix` freshness, and provenance.
- CLI `tsift memory finding-add --text … --anchor … [--kind decision|note] [--confidence c]`.
- **FK-safe anchoring:** the `annotates` edge requires the anchor node to exist; when it doesn't, the authored node is still recorded (carrying `anchor_handle`) and the edge is **deferred** (`anchor_resolved=false`), materializing once the anchor lands. Live-verified both branches.

**`#memgraphrag-ont` — ontology layer** (`tsift-sqlite` + `tsift-cli`)
- `SqliteGraphStore::derive_ontology()` builds a **data-driven** schema from the instance graph: one `ontology_type` node per node-kind + one `ontology_relation:<edge_kind>` edge per observed `(from_kind, edge_kind, to_kind)` triple, each with `instance_count`. Idempotent (excludes existing ontology rows).
- CLI `tsift memory ontology-graph` derives + upserts the layer into `.tsift/graph.db`, so types + permitted relations are queryable alongside instances and retrieval can start from abstract types.
- **Source (your decision):** code `NodeKind`/edge-kind enums + `#trt1` authored types — surfaced empirically over whatever kinds exist. existence-lang foldable in *when a project uses it*; otherwise this efficient data-driven representation stands.

**Verification:** `make check` — clippy `-D warnings` clean, all Rust tests pass (3 new: `authored_node_projection`, `derive_ontology`, `finding-add` defer/resolve). Same single pre-existing red: `opencode-tsift` `npm publish --dry-run` (0.1.63 already published; npm package untouched). `cargo install` + live end-to-end verified (`finding-add` → `decision` node, `ontology-graph` → `ontology_type` derived, no FK errors).

This completes all three MemGraphRAG layers in tsift (decay retrieval + unified graph + ontology backbone) plus authored findings. Queued `#trt1phase2` for the deferred `#trt1` follow-ons (context-pack/search injection, watermark staleness gating, opt-in capture from agent-doc archives, md/html projection). Remaining headline item: `#rankdefault` (unified decay-aware ranking over the merged graph).

### Re: #rankdefault — gated by the code's own quality gate — opus-4-8

Investigated `#rankdefault` to promote it. It is **gated by design in the codebase**, not by my hesitation:

- `graph_db_neighborhood_ranking_gate` (`packages/tsift-cli/src/lib.rs:8104`) hardwires `ranked_output_default: false`, `status: "held_default_order_unchanged"`, with `default_change_gate: "community_search_quality_metrics"`.
- Promotion requires the **community-search quality gate to pass for every required workload** (`COMMUNITY_SEARCH_WORKLOADS`) against thresholds: `min_handle_coverage_pct`, `min_duplicate_name_precision`, `max_duration_regression_percent`, `min_top_community_stability`.
- There is **no automated gate-runner** that produces a pass verdict, and a test (`lib.rs:24441`) asserts the flag stays `false`. The comparison harness exists (`graph_db_ranked_neighborhood_comparison`) but the ranked-vs-unranked **quality study has not been run**.

Flipping `ranked_output_default` to `true` without that study would violate the codebase's explicit contract ("changing the default neighborhood order requires the community-search gate to pass for every required workload") and the item's own condition ("only after quality gates pass"). That's the exact case where gating is correct — so I **gated `#rankdefault`** rather than ship an unmeasured ranking-quality regression.

**Unblock (concrete):**
1. Run the ranked-vs-unranked comparison across `COMMUNITY_SEARCH_WORKLOADS` (real + synthetic) on this repo.
2. Verify all four thresholds pass (handle coverage, duplicate-name precision, duration regression, top-community stability).
3. Only then flip `ranked_output_default` → `true`, update the gate + `lib.rs:24441` test, and integrate the shipped `#memgraphrag1` decay ranking into the unified scoring so memory + code share one ranking.

Stopping the auto-drain here — a gate decision needs your eyeball, and I'd rather not auto-flip a quality-gated default. `#surrealbulk` and `#trt1phase2` remain queued; say the word (or run the quality study) and I'll proceed. I also reordered the backlog so `#trt1phase2` (deferred follow-on) sits below `#rankdefault`/`#surrealbulk` again — my earlier `--pending-add` had prepended it ahead of your groomed priority.
<!-- agent:boundary:2252f2ec -->
<!-- /agent:exchange -->
###
<!--
What are #next-steps to promote SurrealDB to SQLite-equivalent substrate status?
-->

## Queue

<!-- agent:queue preset="#spec-test-build-install-commit-push" go -->
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog priority -->
- [ ] [#surrealbulk] SurrealDB projection performance (consolidates surrealbulk + surrealbulk-close + surreal-batch-bfs + surreal-fixture): replace per-row upsert loops with bulk/transactional row load + one-pass secondary-index rebuild; add batch frontier expansion; record SurrealDB samples in graph-db-performance-history.json fixture; re-run backend-eval benchmarks, update SPEC.md/BENCHMARKS.md table, measure refresh vs load separately.
- [/] [#e2e-install] E2E verify opencode-tsift install in a fresh opencode project: opencode plugin opencode-tsift && opencode then /tsift-status returns healthy (consolidates e2e-install + full-e2e-verify). Live-verify only; npm publish + OIDC auth already done (opencode-tsift@0.1.63 on npm).
- [ ] [#trt1phase2] #trt1 follow-on phases (core authored findings nodes shipped in b300196): context-pack/search injection of authored finding/decision/note nodes; community_graph_watermark staleness gating on finding freshness; opt-in/passive capture of findings from agent-doc session archives; md/html on-demand projection of the findings graph. Builds on shipped authored_node_projection + finding-add + ontology layer.
- [/] [#7qky] [recommended] AGENT-DOC-SCOPED (drive from an agent-doc session, not tsift): re-measure Codex redundant runbook reads after the v0.34.0 retain-dont-reread nudge accumulates live Codex sessions; compare to baseline (Claude 11.4% / Codex 12.6% / OpenCode 5.3%). If Codex rate does not drop, build a session-scoped content-dedup LEDGER in a shared LEAF crate (register(id,hash)/should_deliver->Full|Stub; exemptions via hash-change+compaction; session-id from harness). GATED on accumulating post-nudge Codex sessions.<!-- /agent:backlog -->

## Review

<!-- agent:review -->
- [/] [#rankdefault] Promote ranked_neighborhood into default graph lookup/context-pack/evidence output once quality gates pass; compare ranked vs unranked on real backlog/code queries (useful-hit density, duplicate-name precision, handle coverage, latency, emitted tokens). Also integrate the shipped #memgraphrag1 decay-weighted memory ranking so memory + code share one ranking over the unified graph.
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
