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

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc65443c45f7b49f5e6ace54e3f6bafe4077ade57924422b5eb3baf4dbd23c-20260520-161412.md`*

Compacted content:
- Archived 5 response topic(s): codegraph assessment; local graph database for tsift; standalone SQLite graph substrate vs tsift boundary; 2 more
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc6544...
### Re: tsift graph navigation queue — gpt-5

Completed `#7k0p`, `#15vh`, and `#72v1`.

Implemented in `src/tsift`:

- Route extraction for narrow Rust, TypeScript/JavaScript, and Python web patterns, stored in the SQLite index as `route_nodes` and surfaced in traversal as `route` nodes with `defines_route` and `handled_by` edges.
- Adaptive exploration packets on `traverse` and `context-pack`: project-size-aware budgets, compact relationship maps, line-numbered `source-read` windows, and explicit no-reread guidance for follow-up agents.
- `tsift impact`, an affected-test query over changed files, import lines, indexed call edges, and route handler references, with JSON/human output and runnable test command suggestions.
- Spec and regression coverage for the new route, traversal, exploration, and impact behavior.

Verification in `src/tsift`:

- `cargo check`
- `cargo test` (`613 passed`, `107 passed`, existing wider sim test ignored)
- `make check` (clippy with `-D warnings` plus full test suite)
- `cargo build --release`
- `cargo install --path . --force`
- Installed CLI smoke check: `tsift --version` and `tsift impact --help`

Pushed commits:

- `src/tsift`: `3262277 feat: add route graph and impact packets`
- root repo submodule pointer: `90fd50309 chore: update tsift submodule`

Remote CI: GitHub Actions `CI` for tsift run `26176145686` completed successfully in `2m12s`. There is no tmux-specific workflow or `make tmux-ci` target in the tsift repo; the available CI surface is the `CI` workflow running the full clippy/test suite.
❯ Please review recent changes to root.md. Convex will store the graph data for now using `nodes` + `edges`. Can we abstract our implemnation to support Convex as the data store and maintain the same quality of implementation?
### Re: Convex graph abstraction — gpt-5

Reviewed the uncommitted `src/session-share/tasks/root.md` edits and the current `src/tsift` graph boundary.

Short version: yes. In `tsift`, we can support Convex as a graph data store without lowering implementation quality if Convex is a projection/read model behind the existing provider-neutral graph substrate, not a replacement for local indexing correctness.

What changed in `root.md`:

- Lines 214-216 add business context: ArcadeDB is a hard no, FalkorDB is still possible, and there is an incomplete implementation-time note. The unfinished sentence should be repaired before that doc is used as a decision record.
- Line 226 asks whether Realtime V2V should integrate with AG-UI and whether lower-priority later events can be suppressed. That is a product coordination rule, not graph storage; it belongs in the Orbit/AG-UI PRD.
- Lines 236, 262, 299, and 311-312 add follow-ups: data-driven question count, Robert-facing news/analysis, Google Sheet Column C, and PRDs with Mermaid diagrams. Those are `session-share/root` action items, not `tsift` implementation requirements.

For `tsift`, the good news is the abstraction already exists. The spec explicitly says `src/substrate.rs` is provider-neutral, stores typed nodes/edges/provenance/freshness, and names Convex/FalkorDB as projection targets in [SPEC.md](/home/brian/work/btakita/agent-loop/src/tsift/SPEC.md:12). The concrete model is in [substrate.rs](/home/brian/work/btakita/agent-loop/src/tsift/src/substrate.rs:47): `GraphNode`, `GraphEdge`, `GraphProjection`, and a `GraphStore` trait. The code adapter is already not SQLite-specific: [graph.rs](/home/brian/work/btakita/agent-loop/src/tsift/src/graph.rs:488) projects call edges and routes into generic `code_symbol`, `route`, `calls`, and `handled_by` records. Traversal currently still reads the SQLite index tables directly in [main.rs](/home/brian/work/btakita/agent-loop/src/tsift/src/main.rs:4727), so that is the one important boundary to clean up.

Recommended shape:

1. Keep SQLite as `tsift`'s local/offline/test correctness store. It owns AST extraction, incremental freshness, locks, and deterministic CLI behavior.
2. Add a Convex projection writer/reader that maps the substrate contract onto Convex tables `nodes` and `edges`, with indexes by `kind`, `from_id + kind`, and `to_id + kind`.
3. Keep domain records out of the substrate shape. Orbit topics/questions/answers/assets should become graph node kinds plus properties/provenance, not new Rust structs wired through traversal internals.
4. Make tests backend-agnostic: the same fixture should pass against in-memory SQLite and a mocked Convex adapter using the `GraphStore` contract.
5. Only move graph algorithms to Convex if the product needs reactive server-side queries. For `tsift`, shortest-path/traversal can stay local and deterministic; Convex can serve the app/runtime view.

The quality bar is: stable IDs, idempotent upserts, provenance/freshness on every derived record, typed edge kinds, parity tests, and fail-closed behavior when a projection is stale. If we do that, Convex `nodes` + `edges` is a reasonable current store and still leaves a clean swap path to FalkorDB or another vendor later.

I added a recommended backlog item for the concrete `tsift` implementation step: Convex projection backend behind the provider-neutral substrate, with SQLite preserved as the correctness store and parity tests.
❯ do [#d2k2]. #spec-test-build-install-commit-push
### Re: Convex graph projection backend — gpt-5

Completed [#d2k2].


- Added a shared `GraphProjection::upsert_into` path so stores apply projection nodes before edges.
- Added Convex projection row types for `nodes` and `edges`: stable `externalId` node keys, deterministic `edgeKey` values from `(from_id, kind, to_id)`, and preservation of properties, provenance, and freshness.
- Added a generic `ConvexGraphStore<C>` adapter over a `ConvexGraphClient` contract, including fail-closed edge upserts when either endpoint node is missing.
- Reused the same shortest-path traversal helper for SQLite and Convex-backed stores.
- Updated `SPEC.md` with the Convex `nodes`/`edges` table shape, required indexes, and parity-test expectation.
- Added backend-agnostic projection parity tests proving the same projection round-trips through SQLite and the Convex adapter, plus stable-key and missing-node tests.

Verification:

- `cargo test substrate` passed (`7 passed`).
- `make check` passed (`616 passed`, `107 passed`, `1 ignored`).
- `make ci-full` passed, including the ignored deterministic SimWorld leg.
- `cargo build --release` passed.
- `cargo install --path . --force` completed and replaced the local `tsift` binary.
- Installed smoke check: `tsift --version` returned `tsift 0.1.42`.
- `tsift status --fix` refreshed the local tsift index after the source changes.
- GitHub Actions `CI` run `26178840495` for `btakita/tsift` completed successfully in `2m2s`; the only annotation is GitHub's Node.js 20 action deprecation notice.
- This repo has `CI` and `Release` workflows and no `tmux-ci` make target, so there was no tmux-specific CI leg to inspect.


- `src/tsift`: `48e2630 feat: add convex graph projection adapter`
- root repo submodule pointer: `70087658b chore: update tsift submodule`
<!-- agent:boundary:14a57713 -->
<!-- /agent:exchange -->
###
<!--
-->

<!-- agent:queue -->
dispatch #spec-test-build-install-commit-push
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog -->
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
