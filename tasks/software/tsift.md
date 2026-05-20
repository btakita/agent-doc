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

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc65443c45f7b49f5e6ace54e3f6bafe4077ade57924422b5eb3baf4dbd23c-20260520-194635.md`*

Compacted content:
- Archived 2 response topic(s): graph extension next steps; graph extension implementation
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc6544...
❯ What are #next-steps in support graph db api over sqlite and convex?
### Re: graph DB API next steps — gpt-5

Top backlog item after this cycle: `#gapi`.

The substrate work is past the prototype boundary now: `GraphStore` exists, SQLite materializes `.tsift/graph.db`, `traverse` and `context-pack` read through the store, and Convex has row projection plus snapshot freshness checks. The next step is to make that an explicit graph DB API contract instead of an implementation detail inside traversal.

I added four prioritized backlog items:

1. `#gapi` — promote the provider-neutral graph substrate into a stable graph DB API surface over SQLite and Convex: backend selection, node lookup, kind scans, neighborhood/subgraph reads, shortest paths, JSON schemas, and fail-closed freshness semantics.
2. `#gcon` — add a backend conformance suite so SQLite and Convex prove the same behavior over the same projection fixtures, including ordering, missing-node failures, and stale projection errors.
3. `#cxlv` — add the live Convex transport beyond dry-run planning: auth/config, remote snapshot pull, chunked idempotent upserts, tombstones, retries, resume diagnostics, and schema/index guidance.
4. `#gidx` — harden the SQLite correctness store for incremental projections: version rows, source watermarks, row-level tombstones, migrations, and reader-safe refresh behavior.

I would keep `#98qd` immediately after those. Agent-doc context shaping should consume the stable graph DB API once it exists, rather than binding directly to today’s traversal projection internals.
### Re: graph DB API implementation — gpt-5

Implemented and committed the four queued graph DB items.

- `#gapi`: added a stable provider-neutral graph DB API surface through the expanded `GraphStore` contract and new `tsift graph-db` CLI for schema, node lookup, kind scans, neighborhoods, shortest paths, SQLite reads, and Convex snapshot reads.
- `#gcon`: extended backend conformance coverage across SQLite and Convex row stores for CRUD, ordering, neighborhoods, missing-edge safety, shortest paths, and freshness-gated graph reads.
- `#cxlv`: extended `convex-sync` beyond dry-run planning with `--remote-snapshot`, `--apply`, `--endpoint`, `TSIFT_CONVEX_GRAPH_URL`, bearer-token env support, ordered idempotent chunks, retry receipts, and apply-mode diagnostics.
- `#gidx`: added SQLite graph projection version rows, content/source watermarks, row-level tombstones, schema version checks, and transactional reader-safe projection refreshes.

Spec updates are in `src/tsift/SPEC.md`. Implementation is committed in the tsift submodule as `128d1cd` (`feat: add graph db api and convex sync transport`), and the parent repo submodule pointer is committed as `f42c8c643` (`chore: update tsift submodule`).

Verification:
- `make check` passed (`cargo clippy --all-targets -- -D warnings` plus `cargo test`: 628 unit tests passed, 1 ignored; 107 integration tests passed).
- `cargo install --path .` completed and replaced `/home/brian/.cargo/bin/tsift`; `tsift --version` reports `tsift 0.1.42`.
- Latest GitHub Actions run for `btakita/tsift` was successful: run `26184524562`, job `clippy + test`. That workflow has no tmux-test job listed, so there was no tmux CI leg to remediate.

Push is intentionally after this finalize step per the session closeout ordering.
<!-- agent:boundary:1d8872e3 -->
<!-- /agent:exchange -->
###
<!--
-->

<!-- agent:queue -->
dispatch #spec-test-build-install-commit-push
- do [#gapi]
- do [#gcon]
- do [#cxlv]
- do [#gidx]
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
