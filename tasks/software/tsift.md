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
queue: stop
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

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc65443c45f7b49f5e6ace54e3f6bafe4077ade57924422b5eb3baf4dbd23c-20260608-221432.md`*

Compacted content:
- Archived 8 response topic(s): prompt caching; prompt cache invalidation; prompt cache invalidation; 5 more
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc6544...
- `btakita/agent-loop` `05cd11f10` (`chore: update tsift submodule`)
- `btakita/tsift` `5ca3d6b` (`refactor: extract tsift-cache crate`)
- `cargo install --path . --locked --force`
- `cargo build --workspace`
- `make check` after the release workflow/test updates
- affected release-order tests in `packages/tsift-cli/tests/exit_code.rs`
- `cargo check -p tsift-quality -p tsift-cli`
- `cargo test -p tsift-cli --test cycle_packet_cache`
- `cargo test -p tsift-cache`

### Re: model provider layer and MemGraphRAG boundaries — gpt-5

> **Queue prompt:**
>
> Create a model provider agnostic layer. Does this belong in memgraphrag or a separate layer? What crate is the memgraphrag implementation? Should cache + memgraphrag be in the same crate? If not, explain the dependency relationship.


Create the model-provider-agnostic layer as a separate crate, not inside MemGraphRAG. The best name is probably `tsift-model` or `tsift-llm`: a small adapter contract for model requests, responses, token accounting, batch/sync modes, and provider-specific implementations such as Anthropic/OpenAI. That layer should be below `tsift-summarize`, because `tsift-summarize` is the current crate with live model extraction and it is still Anthropic-specific at `call_anthropic_api`.

There is no single `memgraphrag` crate today. The MemGraphRAG implementation is a direction implemented across these crates:

- `tsift-memory`: memory event schema, first-party `.tsift/memory.db`, decay-weighted retrieval (`#memgraphrag1`), memory-to-graph projection (`#memgraphrag2`).
- `tsift-core`: provider-neutral graph types and the `GraphStore` / `GraphProjection` contract.
- `tsift-sqlite`: the concrete local graph store and ontology derivation (`#memgraphrag-ont`).
- `tsift-cli::commands::memory`: command wiring for `memory project-graph` and `memory ontology-graph`.
- `tsift-summarize`: cached LLM semantic extraction that feeds semantic rows into the graph substrate; this is the piece that needs a model-provider abstraction.

Do not put cache and MemGraphRAG in the same crate. `tsift-cache` should stay a leaf/shared primitive crate. MemGraphRAG is an application/substrate composition over graph + memory + retrieval; cache is infrastructure used by multiple surfaces. Combining them would make unrelated consumers pull memory/RAG concepts just to get disk-cache contracts.

Recommended dependency shape:

- `tsift-cache` has no tsift-domain dependencies.
- `tsift-model` / `tsift-llm` has no memory/RAG dependency; it owns provider adapters and request/response contracts.
- `tsift-summarize` depends on `tsift-model` for model calls and may depend on `tsift-cache` for provider/result cache primitives where appropriate.
- `tsift-memory` depends on `tsift-core` for graph projection, not on `tsift-model` and not on `tsift-cache` unless it later needs a concrete cache primitive.
- `tsift-cli` composes these crates at command boundaries.

So the implementation target is: extract the Anthropic-specific call path from `packages/tsift-summarize/src/summarize.rs` into a provider-neutral model crate, then make `tsift-summarize` call that abstraction. MemGraphRAG should consume the resulting semantic/memory graph rows, not own model-provider logic.

Changed paths this cycle: none. This was an architecture answer after inspecting the current implementation.

Commands used for evidence:

- `tsift status` in `src/tsift`
- `rg` for `memgraphrag`, provider, Anthropic/OpenAI, and model-provider terms
- `tsift --envelope search "memgraphrag model provider agnostic layer" --budget normal`
- source reads for `specs/memgraphrag-direction.md`, `specs/architecture.md`, `packages/tsift-summarize/src/summarize.rs`, and `packages/tsift-memory/src/lib.rs`

Verification: no new code changes in this cycle; prior cache extraction verification remains recorded in the previous response. Confidence is high. Escalation: none.
Yes. MemGraphRAG should live in its own crate.

> **Queue prompt:**
>
> I like tsift-llm. Should the memgraphrag live in its own crate? If yes, extract memgraphrag to its own crate.


I extracted it into `tsift-memgraphrag`, keeping the dependency direction explicit:

- `tsift-memory` now owns durable memory storage, capture/budget contracts, authored finding projection, and claude-mem import/read APIs.
- `tsift-memgraphrag` depends on `tsift-memory`, `tsift-core`, and `tsift-sqlite`; it owns decay ranking, memory query plans, memory-event `GraphProjection` construction, `.tsift/memory.db` upsert into the shared graph store, traversal refresh memory/semantic rows, and ontology materialization.
- `tsift-cli` composes the new crate at command/refresh boundaries.
- The root crate re-exports it as `tsift::memgraphrag`.

This keeps MemGraphRAG separate from the future `tsift-llm` layer: `tsift-llm` should own model-provider request/response/provider adapters, while `tsift-memgraphrag` owns the graph/RAG memory substrate and consumes stored memory/events/semantic rows rather than owning provider calls.

Changed paths:

- Added `packages/tsift-memgraphrag/` with its manifest, README, and library implementation.
- Moved MemGraphRAG APIs out of `packages/tsift-memory/src/lib.rs`: `MemoryDecayConfig`, `ScoredMemoryEvent`, `rank_memory_events`, `MemoryQueryPlan`, `plan_memory_query`, `memory_graph_node_kinds`, and `project_memory_events`.
- Moved CLI-owned graph-store operations into `tsift-memgraphrag`: `project_memory_into_graph`, `derive_memory_ontology_graph`, and traversal memory/semantic row projection.
- Updated `packages/tsift-cli/src/commands/memory.rs` and `packages/tsift-cli/src/lib.rs` to call the new crate.
- Wired the new crate into workspace manifests, release workflow/package-order tests, root re-export tests, `VERSIONS.md`, `specs/architecture.md`, and `specs/memgraphrag-direction.md`.

Commits:

- `src/tsift` submodule: `df1191a` (`refactor: extract tsift-memgraphrag crate`)
- superproject submodule pointer: `3f187f332` (`chore: update tsift submodule`)

Verification:

- `cargo check -p tsift-memgraphrag -p tsift-memory -p tsift-cli`
- `cargo test -p tsift-memgraphrag`
- `cargo test -p tsift-memory`
- `cargo test -p tsift-cli --test exit_code release_publish_gate_requires_secret_variable_and_dry_run`
- `cargo test -p tsift-cli --test exit_code split_crate_manifests_are_publish_ready`
- `cargo test -p tsift-cli --test exit_code cli_manifest_uses_split_crates_without_root_shim`
- `cargo test -p tsift-cli project_memory_into_graph_persists_memory_nodes`
- `cargo test -p tsift-cli traversal_projection_materializes_tsift_memory_rows`
- `cargo test -p tsift --test reexports`
- `cargo package -p tsift-memgraphrag --locked --allow-dirty --list`
- `make check` (passed after the final edit)
- `gh run list --workflow CI --limit 1`: latest CI was `completed success` on `main` for `refactor: extract tsift-cache crate`.
<!-- agent:boundary:88a837a0 -->
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
<!-- /agent:backlog -->

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
