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
<!-- agent:boundary:c3760195 -->
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
