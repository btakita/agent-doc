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

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc65443c45f7b49f5e6ace54e3f6bafe4077ade57924422b5eb3baf4dbd23c-20260601-204619.md`*

Compacted content:
- Archived 2 response topic(s): lazily-rs cache contracts and SurrealDB; SurrealDB parity next steps
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc6544...

### Re: #surreal-backend-eval — gpt-5

> **Queue prompt:**
>
> [#surreal-backend-eval]


Implemented and pushed `surrealdb` as a `graph-db backend-eval` candidate while keeping it held behind the experimental promotion gate.

- Added `GraphDbExperimentalBackend::Surrealdb`, parser aliases (`surrealdb`, `surreal`, `surreal-db`), default candidate inclusion, CLI help, graph-db schema text, adapter-spike listing, and native-adapter-required promotion checks.
- Kept the implementation on the existing provider-neutral read-only row snapshot path so `--candidate surrealdb` loads the same rows and compares signatures against SQLite.
- Extended backend-eval conformance coverage to assert SurrealDB read-only metadata, operation success, SQLite parity, metrics, adapter-spike presence, and hold-gate checks.
- Updated `README.md` and `SPEC.md` so SurrealDB appears in backend-eval candidate/spec surfaces.

Verification passed:

- `cargo test -p tsift-cli cli_parses_graph_db_backend_eval_surrealdb_candidate`
- `cargo test -p tsift-cli --test graph_db_conformance graph_db_backend_eval_benchmarks_candidate_stores_against_sqlite`
- `make check`
- `cargo build`
- `cargo install --path . --force`

`cargo build` and `cargo install` only emitted the existing unused local patch warnings for `agent-kit`, `tagpath`, and `tmux-router`.

CI check: latest GitHub Actions CI run `26769887559` was already red on `main`, but both jobs failed before test steps ran. The annotations on both check runs report GitHub runner `No space left on device` writing runner diagnostics, so there was no CI test/clippy failure to reproduce locally. This repo has only `CI` and `Release` workflows and no `make tmux-ci` target.

Commits pushed:

- `btakita/tsift@074e7b4` (`feat(graph): add surrealdb backend-eval candidate`)
- `btakita/agent-loop@a3576dc3b` (`chore: update tsift submodule`)

### Re: #surreal-parity-suite — gpt-5

> **Queue prompt:**
>
> [#surreal-parity-suite]


Completed `#surreal-parity-suite`.

Implemented explicit SurrealDB backend-eval parity coverage in `packages/tsift-cli/tests/graph_db_conformance.rs`. The conformance test now locates the SQLite and SurrealDB backend reports for every backend-eval dataset and asserts SurrealDB read-only status, empty parity diagnostics, status/count rows, operation `ok` status, SQLite-equivalent row counts, total metrics, per-operation duration metrics, per-1k row-normalized metrics, and row metrics where reported for:

- `status`
- `edge_lookup`
- `edge_property_scan`
- `incident_edges`
- `neighborhood`
- `related`
- `path_max_hops`, `path_max_hops_128`, `path_max_hops_256`, `path_max_hops_512`
- `evidence_target_resolution`
- `evidence`
- `conflict_matrix`
- `dispatch_trace`

Updated `SPEC.md` to document that the compiled conformance suite locks SurrealDB to SQLite-equivalent results and metrics across those backend-eval surfaces.

Verification on the final scoped diff:

- `cargo test -p tsift-cli --test graph_db_conformance graph_db_backend_eval_benchmarks_candidate_stores_against_sqlite` passed (`tart-c46863f05e`).
- `make check` passed (`tart-bd67cf6e93`).
- `cargo build` passed; only the existing unused local patch warnings for `agent-kit`, `tagpath`, and `tmux-router` were emitted.
- `cargo install --path . --force` passed with the same existing unused patch warnings.
- `gh run list --workflow CI --limit 1` showed new CI run `26782385866` in progress for `cd3a445`. The previous run `26781749753` failed before test execution while both jobs were still in the cache step; no failed test step or annotation was available.
- Tmux CI: this repo exposes only `CI` and `Release` workflows, and `make -n tmux-ci` reports no such target.

Pushed commits:

- `src/tsift`: `cd3a445 test(graph): assert surrealdb backend-eval parity`
- parent `agent-loop`: `a07112dc5 chore: update tsift submodule`

### Re: #surreal-native-adapter — gpt-5

Completed `#surreal-native-adapter`.

Implemented a feature-gated optional `tsift-surrealdb` crate, excluded from the default workspace, using the stable SurrealDB Rust SDK 2.x embedded SurrealKV engine. The adapter writes provider-neutral `ConvexProjectionRows` into file-backed SurrealDB records, implements `GraphStore`, and keeps synchronized read indexes so backend-eval parity runs are practical while still proving the SurrealKV projection write/load path.

Wired `backend-surrealdb` through the root and `tsift-cli` feature graph. With the feature enabled, backend-eval replaces the read-only SurrealDB prototype with the SurrealKV adapter under `.tsift/backend-eval-cache/surrealdb/<scope>/<dataset>/surrealkv`; without the feature, the default dependency-free prototype remains in use. Updated docs and conformance coverage accordingly, including order-independent schema tests for serde_json feature-unified column ordering.

Commits pushed:

- `src/tsift`: `2a6d8cb` (`feat(graph): add optional surrealdb adapter spike`)
- parent repo: `0ba7deca0` (`chore: update tsift submodule`)

Verification:

- `cargo check -p tsift-cli --features backend-surrealdb` passed (`tart-bedd8557bb`).
- `cargo test --manifest-path packages/tsift-surrealdb/Cargo.toml` passed (`tart-a0597ee731`).
- `cargo test -p tsift-cli --features backend-surrealdb --test graph_db_conformance graph_db_backend_eval_benchmarks_candidate_stores_against_sqlite` passed (`tart-8f4ef429a1`).
- Default backend-eval conformance passed without the feature (`tart-4b40753551`).
- `make check` passed on the final diff (`tart-9b9dc67030`).
- Default `cargo build` passed (`tart-c6cdc0ac34`) and its log has no `tsift-surrealdb`/SurrealDB compile lines.
- Default `cargo install --path . --force` passed (`tart-e2f44e7862`) and its log has no `tsift-surrealdb`/SurrealDB compile lines.
- GitHub CI: new tsift CI run `26784235057` for `2a6d8cb` is in progress; previous CI run `26782385866` was green.
- Tmux CI: workflows are `CI` and `Release`; this repo has no `make tmux-ci` target.

### Re: #surreal-native-adapter queue repair — gpt-5

Repaired the document closeout state for `#surreal-native-adapter`: the implementation response was already written and committed, and the queue component is now reaped so `#surreal-promotion-gate` is the next live queue item.

### Re: #surreal-promotion-gate — gpt-5

> **Queue prompt:**
>
> [#surreal-promotion-gate]


Ran the SurrealDB native-adapter promotion evidence and left the promotion gate unchanged.

The repeated backend-eval samples completed for the required default workloads, covering real, synthetic high-degree, and synthetic deep-chain datasets:

- `tart-5ccdb7da79`: `for sample in 1 2 3; do target/debug/tsift graph-db --path . --json backend-eval --candidate surrealdb; done | target/debug/tsift metric-digest --baseline fixtures/graph-db-performance-history.json --json --top 8`

The repeated full-projection samples also completed with cache-hit proof:

- `tart-f2fa12fc49`: `for sample in 1 2 3; do target/debug/tsift graph-db --path . --json backend-eval --candidate surrealdb --full-projection; done | target/debug/tsift metric-digest --baseline fixtures/graph-db-performance-history.json --json --top 8`
- Latest full-projection cache metrics: `full_projection.cache.hit=1`, `full_projection.cache.disk_bytes=3233926`, `full_projection.cache.compression_ratio=0.21222778981092985`.

SurrealDB preserves the backend-eval measurement surface, but it does not beat SQLite on the required workloads, so it is not promotion-eligible:

- Default real total: SQLite `5611us`, SurrealDB `22671443us`.
- Default synthetic high-degree total: SQLite `42289us`, SurrealDB `712114us`.
- Default synthetic deep-chain total: SQLite `45001us`, SurrealDB `1599612us`.
- Full-projection total: SQLite `1821531us`, SurrealDB `22924553us`.
- Full-projection required operations also miss the speed gate, including `neighborhood` (`4100us` SQLite vs `2087997us` SurrealDB), `path_max_hops` (`194us` vs `291734us`), `evidence` (`234us` vs `69542us`), and `conflict_matrix` (`85us` vs `26997us`).

Install portability evidence from the adapter cycle still applies: default `cargo build` (`tart-c6cdc0ac34`) and default `cargo install --path . --force` (`tart-e2f44e7862`) completed without pulling SurrealDB into the default dependency graph. The backend-eval report still records the multi-process/read-only lock semantics requirement as a promotion hold for SurrealDB; since the repeated timing gate already fails, I did not add a new promotion-gate edit.

No repository files changed for this queue item. Latest GitHub Actions CI for `btakita/tsift@2a6d8cb` is green (`26784235057`). Marked `#surreal-promotion-gate` done.
<!-- agent:boundary:8b7f9f18 -->
<!-- /agent:exchange -->

## Queue

<!-- agent:queue -->
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog -->
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
