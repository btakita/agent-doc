---
agent_doc_session: tsift-v0.1
resume: 019dd526-7958-77e3-9a41-92b8944fc550
agent: codex
claude_model: opus
agent_doc_format: template
agent_doc_write: crdt
claude_args: --dangerously-skip-permissions
codex_args: -s danger-full-access
prompt_presets:
  '#agent-doc-bug': Please create a plan for agent-doc to fix this issue. Add to the backlog of agent-loop/tasks/agent-doc/agent-doc-bugs2.md
  '#code-review': Please review tsift codebase. Inspect the architecture and look for edge cases. Isolate concurrency issues and other workflow bugs. Let's find root causes to fix. '#follow-up-backlog'
  '#next-steps': Any follow-up items to place in the backlog? Please ensure the backlog is in priority order. Print the item id of the top of the backlog.
  '#spec-test-build-install-commit-push': update spec + tests. build + install for local testing. commit + push
  '#spec-test-commit-push': update spec + tests. commit + push
---

## Status

<!-- agent:status patch=replace -->
Project created. Repo: https://github.com/btakita/tsift. Submodule: src/tsift. Spec extracted to src/tsift/SPEC.md.
<!-- /agent:status -->

## Spec

See [src/tsift/SPEC.md](../../src/tsift/SPEC.md) for the AST-Aware Code RAG spec.

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc65443c45f7b49f5e6ace54e3f6bafe4077ade57924422b5eb3baf4dbd23c-20260520-144729.md`*

Compacted content:
- Archived 2 response topic(s): #kgnv Graphify-style traversal; #fidx fresh index gate
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc6544...
❯ Please clone and assess https://github.com/colbymchenry/codegraph. Are there any patterns & tech in this project that are useful for tsift?
### Re: codegraph assessment — gpt-5

I cloned `https://github.com/colbymchenry/codegraph` into `/tmp/codegraph-assess.JOSqXU/codegraph` and assessed commit `79b9601` against local tsift commit `3aebc7e`.

Short answer: yes, there are useful patterns, but they should be ported selectively. CodeGraph is an MCP-first TypeScript knowledge-graph product; tsift is already stronger on binary-owned agent workflows, freshness gates, bounded handles, and SQLite lock recovery. I would borrow product and graph-query patterns, not the runtime architecture wholesale.

Most useful:

- **Adaptive exploration packets.** CodeGraph's `codegraph_explore` scores entry-point symbols, groups related nodes by file, emits relationship maps, reads clustered line-numbered source windows, and scales output budgets by project size. This maps cleanly onto tsift `context-pack`, `traverse`, `source-read`, and `workflow search`; tsift already has stable handles and envelopes, so the missing piece is better relevance clustering and file-section packing.
- **Framework/route nodes.** CodeGraph models web routes as first-class `route` nodes linked to handlers through `references` edges across several frameworks. This is directly useful for tsift because real agent questions often start at endpoints, UI routes, commands, or workflow names instead of pure function names. I would start narrow with Rust, TypeScript, and Python route extractors.
- **Affected-test / impact analysis.** CodeGraph has `affected` and symbol impact surfaces for changed files. tsift already has search, graph, digests, and call edges; a `tsift affected`-style command could turn changed files into likely tests, affected symbols, and closeout commands for agent-doc/CI workflows.
- **Concise MCP/tool-selection instructions.** CodeGraph emits server instructions telling agents when to use search, context, callers, impact, node, files, and status. tsift's equivalent should bias agents toward existing safe primitives: `search`, `explain`, `source-read`, `context-pack`, `session-review`, and `workflow search`.
- **Evaluation harness shape.** CodeGraph's evaluation cases assert that realistic exploration prompts surface expected symbols, not just that parsers produce nodes. That pattern is worth adding to tsift's DCI/search-quality fixtures: query -> expected symbol/file/handle coverage.

Less useful for tsift:

- The Node/TypeScript runtime, `web-tree-sitter`/WASM packaging, and optional `better-sqlite3` fallback do not fit tsift as well as the current Rust CLI plus feature-gated tree-sitter grammars.
- CodeGraph's database concurrency story is weaker than tsift's current WAL, sidecar writer lock, and snapshot fallback model. Keep tsift's lock/freshness design.
- Always-on file watching is only partly transferable. For agent-doc cycles, tsift's explicit fresh-index gate is safer than assuming a watcher caught up.
- FTS5 over symbol metadata is worth noting, but I would not prioritize it until `sift` plus exact `rg -F` leaves a demonstrated retrieval gap.

I added three recommended backlog items for the parts that look actionable: adaptive exploration packets, framework/route graph nodes, and affected-test/impact analysis.
❯ Would tsift benefit from using a true local graph database over sqlite?
### Re: local graph database for tsift — gpt-5

Short answer: not yet. I would keep SQLite as tsift's authoritative store and add graph-database-like query surfaces over it before introducing a second embedded graph backend.

The current tsift graph workload is still a good fit for SQLite: symbol rows, indexed `call_edges`, caller/callee lookups, distinct edge scans, BFS, community detection, traversal slices, and bounded agent packets. The harder parts of tsift are freshness, per-submodule isolation, transactionality, WAL behavior, sidecar writer locks, snapshot fallback, and predictable local packaging. A true local graph database would mostly add another operational boundary before tsift has proven that SQLite plus in-memory traversal is the bottleneck.

A graph DB becomes worth reconsidering only if tsift starts needing persistent multi-hop pattern matching over richer typed edges at scale, for example constrained route-to-handler-to-symbol-to-test traversals across very large monorepos, and measurements show SQL plus in-memory traversal is too slow or too awkward. Even then, I would prototype it as a derived read-only cache rebuilt from SQLite, not as the source of truth.

For now, the better direction is to keep the reliability model and make the SQLite graph model more graph-shaped when needed: typed node/edge tables, indexes around source/destination/kind, and possibly materialized paths or closure tables for the handful of traversals agents ask for repeatedly. That should cover most graph-product ergonomics without giving up the concurrency and recovery behavior tsift already has. I would treat this as guidance for the existing graph/route/adaptive-packet backlog items rather than adding a separate database-migration item.
❯ Can our work to make a graphdb backed by sqlite be abstracted into a standalone package for any sort of content? Such that it can support from /home/brian/work/btakita/agent-loop/src/session-share/tasks/root.md Knowledge Navigator or Orbit or any BuildParty agents?

❯ Or would tsift be the correct abstraction that encloses the sqlite-backed graphdb?
### Re: standalone SQLite graph substrate vs tsift boundary — gpt-5

Yes, the SQLite-backed graph work can be abstracted, but I would draw the boundary lower than tsift.

The reusable package should be a small local typed property graph substrate: nodes, edges, labels/kinds, properties, provenance/source refs, content hashes, freshness timestamps, aliases, FTS/vector hooks, path/neighborhood/subgraph queries, WAL/snapshot/lock behavior, and rebuildable projections. It should not know about code symbols, ASTs, LiveKit, Orbit, semantic heroes, or agent-doc. That makes it usable for source code, markdown sessions, docs, issues, videos, semantic tags, enhanced content, facts, and admin registries.

Then tsift becomes one opinionated product built on top of that substrate: it owns code ingestion, tree-sitter extraction, files/symbols/routes/call edges, `source-read`, `context-pack`, `traverse`, freshness gates, and agent-facing code retrieval. That is the right abstraction for code and repo navigation, but it is too narrow to be the enclosing abstraction for Knowledge Navigator, Orbit modules, or BuildParty's agent graph.

For BuildParty/Orbit specifically, the local SQLite graph should be treated as a dev/offline/edge/rebuildable graph substrate or ingestion artifact, not the platform runtime SoR. The current BuildParty database strategy already points production durable state at Postgres and runtime graph queries at FalkorDB. A standalone SQLite graph package can still be valuable there: it can power local module builds, deterministic tests, portable knowledge packs, content-ingestion staging, and small deployments. Production can project the same generic node/edge/provenance model into Postgres/FalkorDB.

So the clean layering is:

1. **Generic graph substrate** — SQLite-backed typed property graph library/CLI, content-agnostic.
2. **tsift** — code intelligence product and CLI using the substrate for source-code graphs.
3. **agent-doc / session tooling** — session/backlog/document graph adapters using the same substrate.
4. **BuildParty / Orbit / Knowledge Navigator** — product agents and content modules that may import or project from the substrate, but are not enclosed by tsift.

I would not extract immediately as a polished public package. First, define the graph schema/API inside tsift with an explicit `no code-domain concepts below this line` boundary. Once a second real consumer lands, probably agent-doc session graphs or an Orbit module pack, extract that layer into a standalone crate/package. That avoids designing an abstract graph library in the dark while still keeping the implementation ready to split.
❯ I want to make some notes:
❯ - The Orbit agent will include looking at code bases for software logic + comment extraction if the codebase is available.
❯ - FalkorDB will be considered but there are reservations on using FalkorDB. So for now, the logic from tsift and/or extracted library may serve as a prototype for graph behavior.

❯ Please reread root.md as a turn has ended. Reassess.
### Re: graph substrate reassessment after root.md — gpt-5

After rereading `root.md`, I would revise the earlier answer slightly, not reverse it.

The important change is that Orbit is not just a content/agent consumer. It may inspect codebases, extract software logic, and mine comments when source is available. That makes tsift a natural Orbit capability, but still not the enclosing abstraction. tsift should own code intelligence: AST extraction, symbol/call/route graphs, code comments, source windows, repo freshness, and agent-safe code packets. Orbit should call, import, or project from tsift for code-aware knowledge, not become a tsift subproduct.

The graph substrate should be provider-neutral. SQLite is still the right first implementation because it gives local determinism, portable packs, rebuildable projections, CI fixtures, and the reliability behavior tsift already needs. But after `root.md`, I would explicitly design the substrate as a canonical graph model with projections:

- SQLite: local/offline/dev/test graph store and the first implementation.
- Convex: reactive product/app knowledge store, useful for Orbit UI sync, taxonomy/topic/question/answer/asset records, vector/full-text search, and session state.
- Optional graph backend: derived read model for production multi-hop graph queries if Convex queries become awkward or slow.

Convex should not be treated as a true graph database. Its official docs describe a document-relational reactive database plus full-text/vector search, not native graph traversal or pattern matching: [Database](https://docs.convex.dev/database), [AI & Search](https://docs.convex.dev/search), [Vector Search](https://docs.convex.dev/search/vector-search). So I would not build "a graph layer on top of Convex" as the core abstraction. I would store graph-shaped records in Convex when the product needs reactive state, but keep graph traversal semantics in the provider-neutral graph layer.

If production Orbit needs fast multi-hop queries, add a graph backend as a replaceable projection, not as the source of truth. FalkorDB can still be evaluated, but the seed-stage/company-risk concern argues for avoiding hard coupling. The contract should be "generic graph model to projections" rather than "FalkorDB everywhere" or "Convex pretending to be a graph DB."

So the updated layering is:

1. Provider-neutral typed graph substrate: content-agnostic nodes, edges, labels/kinds, provenance, source refs, freshness, embeddings/FTS hooks, and traversal/query APIs.
2. SQLite implementation: first-class local engine and prototype/test fixture.
3. tsift adapter: codebase ingestion, AST symbols, comments, routes, call/import edges, source-read/context-pack/traverse.
4. Orbit/Knowledge Navigator adapters: content, docs, LiveKit knowledge, questions/answers/assets, and optional codebase ingestion through tsift.
5. Convex/FalkorDB projections: deployment choices for reactive product state or production graph reads.

That means `#bahy` is still the right top backlog item, but it should name Convex and optional graph-backend projection explicitly. The risk to avoid is extracting too early into a generic "any content graph DB" package before tsift and Orbit have forced the schema through real adapters. I would keep the first implementation inside or next to tsift, draw the no-code-domain boundary now, and extract once the Orbit code/comment adapter or agent-doc session graph becomes the second real consumer.
<!-- agent:boundary:72ce05d4 -->
<!-- /agent:exchange -->
###
<!--
-->

## Backlog

<!-- agent:backlog -->
- [ ] [#bahy] [recommended] Spike a provider-neutral typed property graph substrate behind tsift: SQLite as the local/offline prototype and test engine, generic nodes/edges/provenance/freshness/query APIs, tsift code/comment extraction as one adapter, Convex projection for Orbit's reactive knowledge store, and optional graph-backend projection such as FalkorDB as a replaceable derived read model rather than tsift owning Orbit/BuildParty.
- [ ] [#7k0p] [recommended] Borrow CodeGraph-style adaptive exploration packets for tsift context/traverse surfaces: project-size-aware budgets, relationship maps, line-numbered clustered source windows, and no-reread guidance.
- [ ] [#15vh] [recommended] Add framework/route nodes to the tsift graph, starting with narrow Rust/TypeScript/Python route extractors that emit route nodes and handler reference edges.
- [ ] [#72v1] [recommended] Add a tsift affected-tests/impact query over changed files, imports, and call edges so closeout and CI loops can choose likely test targets without broad manual scanning.
- [ ] [#98qd] Support agent-doc context shaping/orchestration: add tsift-backed backlog coverage for job packets, source handles, and bounded worker context.
- [ ] [#atvp] Unblock crates.io publication for tsift by replacing the upstream git-only sift dependency, publishing a compatible upstream crate, or vendoring a small stable search adapter; rerun cargo publish --dry-run before enabling TSIFT_ENABLE_CRATES_PUBLISH.
- [ ] [#g6kf] Confirm the v0.1.42 GitHub release completes after the macOS x86_64 Actions runner leaves the queue; if it remains stuck, remove or replace that matrix target and retag a fixed release.
<!-- /agent:backlog -->

<!-- agent:icebox -->
- [ ] [#9jc8] graphify integration: LLM-based entity extraction for semantic layer beyond AST
- [ ] [#zv4m] graphify integration: HTML visualization of symbol/dependency graphs
- [ ] [#rd2f] graphify integration: semantic similarity between concepts (embedding-based)
<!-- /agent:icebox -->

## Completed / Reaped

<!-- agent:done archive=tasks/software/tsift.done.md -->

<!-- completed work archived in tasks/software/tsift.done.md -->
<!-- /agent:done -->
