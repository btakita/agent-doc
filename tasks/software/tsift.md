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

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc65443c45f7b49f5e6ace54e3f6bafe4077ade57924422b5eb3baf4dbd23c-20260607-021442.md`*

Compacted content:
- Archived 4 response topic(s): msgpack instead of JSON for serialization; #3dhp cache-encoding; Can tsift become a mem graph RAG? (arxiv 2606.00610); 1 more
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc6544...

### Re: Run the gated backlog items — opus-4-8

> **Queue prompt:**
>
> run the gated backlog items


Both gated items are implemented, tested, built/installed, committed, and pushed (`btakita/tsift` `main` `188bd5b`). Marked `#memgraphrag1` and `#memgraphrag2` done.

**`#memgraphrag1` — decay-weighted memory retrieval** (`packages/tsift-memory/src/lib.rs`)
- `MemoryDecayConfig { half_life_secs, lexical_weight, recency_weight }` — default 1-week half-life, 0.6/0.4 lexical/recency blend.
- `rank_memory_events(events, query, now_unix, config, limit) -> Vec<ScoredMemoryEvent>` — blends lexical overlap with exponential recency decay `0.5^(age/half_life)` over `observed_at_unix`; no-timestamp events keep lexical score, earn no recency; ties break toward recent. `ScoredMemoryEvent` exposes component scores.
- `MemoryQueryPlan` now carries the `decay` contract (visible in `tsift memory query-plan --json`).
- 4 unit tests (decay halves at half-life, recency tiebreak, strong-lexical-beats-recency + limit, plan carries config).

**`#memgraphrag2` — one graph, not two** (`packages/tsift-cli/src/commands/memory.rs`)
- New `tsift memory project-graph [PATH] [--graph-db <p>] [--limit N]` reads stored memory events, projects them (`memory_session`/`memory_event` nodes + `records_memory_event` edges), and upserts into the shared `.tsift/graph.db` via the existing `SqliteGraphStore::upsert_projection` — memory nodes are now queryable alongside code symbols.
- Persistence round-trip test (insert events → project → reopen graph.db → assert `memory_event`/`memory_session` rows).

**Verification:**
- `make check`: clippy `-D warnings` clean, **all Rust tests pass** (incl. the 5 new ones). The single red is `opencode-tsift`'s `npm publish --dry-run` rejecting because `0.1.63` is **already published** — a pre-existing release-state condition in the npm package (untouched by this change), not a code regression; it fails identically at HEAD.
- `cargo install --path .` done; live-verified both: `query-plan` reports the decay config, `project-graph` runs against the real graph store.

**Scope notes:** staged only my 4 files (left the submodule's pre-existing dirty `AGENTS.md`/`.opencode/`/`BENCHMARKS.md` untouched). Did **not** bump the version or publish — the preset was build+install+commit+push, not release; a publish would need a coordinated version bump (which also clears the `publish:check` red). The agent-loop superproject's `src/tsift` pointer now trails `188bd5b` if you want to bump it.

Remaining toward full MemGraphRAG: `#trt1` (authored Finding/Decision/Note node types) and folding decay ranking into `#rankdefault` over the now-merged graph.

### Re: Commit all uncommitted tsift changes — opus-4-8

> **Queue prompt:**
>
> Commit all uncommitted tsift changes.


Committed and pushed (`btakita/tsift` `main` `232c766`). The tsift submodule working tree is now clean. The 14 uncommitted files were all generated/doc surfaces (no source code):

- **`AGENTS.md`** — `tsift init` code-navigation block refresh (marker `v0.1.62` → `v0.1.63`, added `source-read`/`symbol-read` entries).
- **`.opencode/commands/*.md`** (12 files) — opencode command shortcuts from `tsift init --opencode`.
- **`BENCHMARKS.md`** — graph-db `backend-eval` results (SQLite vs SurrealDB).

Pushed to keep `main` consistent with the already-pushed feature commit `188bd5b`. Note: the `.opencode/commands/*.md` still carry `v=0.1.62` markers while `AGENTS.md` is at `v0.1.63` — a minor drift from a partial `tsift init` run. I committed them as-is per your request; re-run `tsift init --opencode` if you want the markers realigned.

(The agent-loop superproject's `src/tsift` pointer now trails `232c766`.)

### Re: Ontology / Fact / Passage Graph mapping onto tsift — opus-4-8

> **Queue prompt:**
>
> How does the https://arxiv.org/pdf/2606.00610 Ontology Graph, Fact Graph, and Passage Graph map onto tsift?


The paper bundles these as the **Hierarchical Indexing Graph (𝒢)**, three layers that retrieval traverses top-down (abstract → grounded): Semantic Ontology Graph → Fact Graph → Source Evidence Graph. Here's how each lands on tsift:

| Paper layer | What it holds | tsift analog | Coverage |
|---|---|---|---|
| **Semantic Ontology Graph** (𝒢_ont) — schema *types* + valid relations between types; the "logical backbone" | tsift's **node-kind / edge-kind schema** (`NodeKind`, `memory_graph_node_kinds()`, edge kinds like `calls`/`references`/`records_memory_event`) | ⚠️ **partial** — kinds exist but only as *conventions on rows*, not first-class queryable type-nodes with inter-type relations. Communities are emergent clusters, not a typed ontology. **This is the real gap.** |
| **Fact Graph** (𝒢_fac) — concrete entities + instantiated relation triples, multi-hop, bridging edges | tsift's **`GraphStore`**: code symbols = entities; call/reference/import edges = triples; `+` memory_event/memory_session nodes after `#memgraphrag2`. Multi-hop via `graph --callers/--callees`, `explain`, `ranked_neighborhood` | ✅ **strong** — this is tsift's core |
| **Source Evidence Graph** (𝒢_pas) — passages + links from facts/entities back to originating text | tsift's **AST spans + provenance**: every node carries `source_ref` / `GraphProvenance` / file:line span; `source-read`/`symbol-read` expand to grounded text; BM25 `token-index` is the passage retrieval surface | ✅ **strong** — "every result is a citable handle" is exactly this layer |

```mermaid
flowchart TD
  subgraph Paper["MemGraphRAG Hierarchical Indexing Graph"]
    O[Ontology Graph<br/>schema types] --> F[Fact Graph<br/>entities + triples]
    F --> P[Source Evidence Graph<br/>passages]
  end
  subgraph tsift["tsift substrate"]
    OK[node/edge KIND schema<br/>⚠️ implicit, not materialized] --> GS[GraphStore<br/>symbols + edges + memory nodes]
    GS --> EV[AST spans + provenance<br/>+ BM25 token-index]
  end
  O -. maps .-> OK
  F -. maps .-> GS
  P -. maps .-> EV
```

**Cross-layer linkages:**
- **fact ↔ passage grounding** — tsift's strongest property. Every symbol/node resolves to a file:line span with provenance; `source-read` makes every reasoning step traceable to text. This is the 𝒢_pas link, already first-class.
- **schema ↔ instance alignment** — only half-present. Each node has a `kind`, so instance→type is recoverable, but the *type itself* isn't a node and there's no "valid relations between types" graph to constrain traversal. The paper uses 𝒢_ont as a logical backbone; tsift has no backbone, just typed rows.

**The one genuine gap = the Ontology Graph.** To match MemGraphRAG, tsift would materialize an ontology layer: node-kind and edge-kind *types* (and their permitted relations) as a small queryable schema graph, letting retrieval start from abstract types and prune by schema constraints. The ontology source could be the code's `NodeKind`/edge-kind enums, the authored-node types from `#trt1`, or your Existence Lang ontology. Queued as gated `#memgraphrag-ont` (needs a decision on the ontology source).

Net: tsift already implements the bottom two of the three layers well — it's a **Fact + Evidence** graph today. The missing piece to be a full tri-layer MemGraphRAG is the typed ontology backbone on top.

### Re: Triage review → backlog, consolidate, prioritize — opus-4-8

> **Queue prompt:**
>
> Please look at the review items. Move actionable steps into the backlog. Create new backlog items and/or Consolidate items if it makes sense. After backlog items are in remove the review items. Use auto-dag to prioritize. re [#memgraphrag-ont]: I want to use #trt1 and code NodeKind/edge-kind enums to be the source of the ontology. The source may include existence-lang if used in the project(s). But existence-lang is not always used. If a dynamically generated existence-lang ontology is useful as a basis for the ontology layer, use it. Otherwise, we can use an efficient representation for the MemGraphRag.


Done. Reviewed all 13 review items, consolidated into a curated 6-item backlog, **emptied the review block**, and prioritized via `auto-dag`. The 13 originals were reaped out (archived).

**Dropped as already done** (memory: opencode-tsift is published via OIDC trusted publishing, `NPM_TOKEN` deleted; `opencode-tsift@0.1.63` is on npm):
- `#0y9e`, `#astnav-npm`, `#npm-auth-setup`, `#npm-publish-verify` — npm auth + publish complete.

**Consolidated:**
- `#surrealbulk` now absorbs `#surrealbulk-close` + `#surreal-batch-bfs` + `#surreal-fixture` → one SurrealDB-perf item (bulk load + frontier + fixture + benchmark closeout).
- `#e2e-install` absorbs `#full-e2e-verify` → one fresh-project verification item.

**Kept actionable:** `#trt1` (findings graph layer), `#rankdefault` (now also folds in the shipped `#memgraphrag1` decay ranking), `#7qky` (gated — flagged **agent-doc-scoped**, should be driven from an agent-doc session, not tsift).

**`#memgraphrag-ont` — ungated + source decided** per your call: base ontology = **code `NodeKind`/edge-kind enums + `#trt1` authored-node types**; optionally fold in a **dynamically-generated existence-lang ontology when a project actually uses existence-lang** (not always present); otherwise an efficient internal representation. No longer gated.

**Prioritized order** (auto-dag lanes + dependency: foundational first):

```mermaid
flowchart TD
  T[#trt1 findings graph layer] --> O[#memgraphrag-ont ontology layer]
  T --> R[#rankdefault + memgraphrag1 decay]
  O --> R
  S[#surrealbulk SurrealDB perf]
  E[#e2e-install opencode verify]
  Q[#7qky codex reads · gated · agent-doc-scoped]
```

1. `#trt1` — foundational (authored nodes; unblocks the fuller ontology + unified ranking)
2. `#memgraphrag-ont` — ontology backbone (builds on #trt1)
3. `#rankdefault` — unified decay-aware ranking over the merged graph
4. `#surrealbulk` — SurrealDB projection perf (independent)
5. `#e2e-install` — live verification (independent)
6. `#7qky` — gated, agent-doc-scoped (last)

Review block is now empty; backlog is the single source of actionable work.
<!-- agent:boundary:7e63c53b -->
<!-- /agent:exchange -->
###
<!--
What are #next-steps to promote SurrealDB to SQLite-equivalent substrate status?
-->

## Queue

<!-- agent:queue preset="#spec-test-build-install-commit-push" go -->
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog priority queue -->
- [ ] [#trt1] Findings graph layer: add authored Finding/Decision/Note nodes to GraphStore, anchored by stable symbol-handle/tagpath edges (not line numbers), provenance+confidence+freshness gated on community_graph_watermark for staleness; retrieval via context-pack/search injection + a /graph-menu command; opt-in/passive capture from agent-doc session archives; graph store as source of truth, md/html as on-demand projection. Foundational for #memgraphrag-ont and #memgraphrag2. Spec: src/tsift/specs/memgraphrag-direction.md + SPEC.md.
- [ ] [#memgraphrag-ont] Materialize a Semantic Ontology Graph layer (MemGraphRAG arxiv 2606.00610 third layer) so node/edge KINDS become first-class queryable type-nodes with permitted inter-type relations, letting retrieval start from abstract types and prune by schema. ONTOLOGY SOURCE (decided): code NodeKind/edge-kind enums + #trt1 authored-node types as the base; optionally fold in a dynamically-generated existence-lang ontology when a project actually uses existence-lang (not always present) -- otherwise fall back to an efficient internal representation. Builds on #trt1; relates to #memgraphrag2.
- [ ] [#rankdefault] Promote ranked_neighborhood into default graph lookup/context-pack/evidence output once quality gates pass; compare ranked vs unranked on real backlog/code queries (useful-hit density, duplicate-name precision, handle coverage, latency, emitted tokens). Also integrate the shipped #memgraphrag1 decay-weighted memory ranking so memory + code share one ranking over the unified graph.
- [ ] [#surrealbulk] SurrealDB projection performance (consolidates surrealbulk + surrealbulk-close + surreal-batch-bfs + surreal-fixture): replace per-row upsert loops with bulk/transactional row load + one-pass secondary-index rebuild; add batch frontier expansion; record SurrealDB samples in graph-db-performance-history.json fixture; re-run backend-eval benchmarks, update SPEC.md/BENCHMARKS.md table, measure refresh vs load separately.
- [ ] [#e2e-install] E2E verify opencode-tsift install in a fresh opencode project: opencode plugin opencode-tsift && opencode then /tsift-status returns healthy (consolidates e2e-install + full-e2e-verify). Live-verify only; npm publish + OIDC auth already done (opencode-tsift@0.1.63 on npm).
- [/] [#7qky] [recommended] AGENT-DOC-SCOPED (drive from an agent-doc session, not tsift): re-measure Codex redundant runbook reads after the v0.34.0 retain-dont-reread nudge accumulates live Codex sessions; compare to baseline (Claude 11.4% / Codex 12.6% / OpenCode 5.3%). If Codex rate does not drop, build a session-scoped content-dedup LEDGER in a shared LEAF crate (register(id,hash)/should_deliver->Full|Stub; exemptions via hash-change+compaction; session-id from harness). GATED on accumulating post-nudge Codex sessions.<!-- /agent:backlog -->

## Review

<!-- agent:review -->
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
