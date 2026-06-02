---
agent_doc_session: tsift-v0.1
resume: 019e85d7-f9ee-79f3-b597-4ed352495bbd
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

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc65443c45f7b49f5e6ace54e3f6bafe4077ade57924422b5eb3baf4dbd23c-20260601-222220.md`*

Compacted content:
- Archived 5 response topic(s): #surreal-backend-eval; #surreal-parity-suite; #surreal-native-adapter; 2 more
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc6544...
❯ Is surrealdb at parity with sqlite?

### Re: SurrealDB parity — gpt-5

Short answer: yes for the current backend-eval parity surface, no for production replacement parity with SQLite.

The conformance path asserts SurrealDB's backend-eval report has `parity.matches_sqlite == true` and no diagnostics for each dataset, then compares SurrealDB row counts against SQLite for the parity operations. The spec says the SurrealDB prototype is compared against SQLite across status/count rows, edge lookup, edge-property scan, incident scan, neighborhood, related retrieval, path tiers, evidence, conflict-matrix, and dispatch-trace.

But that is still backend-eval/prototype parity. It is not promoted to SQLite-equivalent substrate status. The promotion gate keeps SurrealDB behind backend-eval until a real optional adapter proves provider-neutral projection writes/load, full `GraphStore` parity, writer/read-only lock behavior, default build/install portability, full-projection wins, and faster-than-SQLite results across the real, full-projection, high-degree, and deep-chain workloads. The code also labels the default SurrealDB path as a read snapshot unless the `backend-surrealdb` feature is enabled.

So I would treat it as: `SurrealDB is parity-checked and currently passes the measured read/query parity suite; SQLite remains the stable baseline and default until SurrealDB passes the production adapter and performance gates.`

Evidence checked: `SPEC.md`, `packages/tsift-cli/src/lib.rs`, and `packages/tsift-cli/tests/graph_db_conformance.rs`. No repo files changed.
❯ What is the performance of SurrealDB compared to SQLite?

### Re: SurrealDB performance — gpt-5

I ran the local backend-eval comparison with `--candidate surrealdb`, including the opt-in `--full-projection` gate. The result is: SurrealDB still passes parity, but it does not beat SQLite overall in the promotion-relevant workloads.

Latest `--full-projection` totals:

| dataset | SQLite | SurrealDB | result |
| --- | ---: | ---: | --- |
| real | 17.6 ms | 375.2 ms | SurrealDB ~21x slower |
| synthetic_high_degree | 17.6 ms | 9.2 ms | SurrealDB ~1.9x faster |
| synthetic_deep_chain | 15.7 ms | 6.4 ms | SurrealDB ~2.5x faster |
| full_projection | 38.4 s | 50.7 s | SurrealDB ~1.3x slower |

The important detail is the operation mix. SurrealDB wins some synthetic totals because its prototype load/refresh path is cheaper there, and it is faster on some deep-chain path/evidence-style operations. But it loses badly on the operations that matter for replacing SQLite on the real/full graph: full-projection `edge_lookup` was 4.30 s vs SQLite 13.2 ms, `edge_property_scan` was 2.00 s vs 27.7 ms, `incident_edges` was 3.62 s vs 0.39 ms, `related` was 24.5 s vs 19.3 ms, and path probes were seconds vs tens of microseconds.

So the current conclusion is: `SurrealDB is parity-correct but not performance-competitive enough to promote.` The backend-eval promotion decision remains `hold`, with the explicit gate still requiring a real optional adapter, provider-neutral projection writes/load, SQLite parity on real and full-projection datasets, matching-or-better lock behavior, install portability, and faster-than-SQLite results across the required workloads.

Commands run: `tsift status`, `agent-doc preflight`, `agent-doc plan`, `tsift graph-db --path /home/brian/work/btakita/agent-loop/tasks/software/tsift.md --json backend-eval --candidate surrealdb --target gval`, and the same backend-eval command with `--full-projection`. No repo code files were changed.
❯ Can SurrealDB be optimized for performance? We put in substantial effort to optimize Sqlite.

### Re: SurrealDB optimization potential — gpt-5

Yes, SurrealDB can probably be optimized a lot from where it is now. The current numbers are not a fair apples-to-apples comparison with the SQLite path, because SQLite has received substantial workload-specific optimization and the SurrealDB adapter is still a feature-gated spike.

The main issue I found is structural: `SurrealdbGraphStore` persists rows into SurrealDB, but its hot reads mostly go through two Rust `BTreeMap`s keyed by node id and edge id. It overrides `outgoing_edges`, but it does not override several high-impact `GraphStore` methods where SQLite has dedicated indexed implementations. The default trait methods for `edge`, `incident_edges`, `paged_incident_edges`, and `edges_between_nodes` fall back to `all_edges()` / repeated `outgoing_edges()` scans. On the full projection, that means some SurrealDB benchmark operations are effectively paying O(E) or repeated scan costs over ~657k edges.

SQLite is not just “using SQLite.” It has very specific pushdown work: `edge()` probes `idx_graph_edges_edge_key`, incident scans use separate `from_id` and `to_id` index probes, property scans use materialized `graph_edge_properties`, `edges_between_nodes()` uses a temp-table + `EXISTS` pattern, and path reads use a bounded frontier over `idx_graph_edges_from_kind`. Those are exactly the operations where SurrealDB is losing by orders of magnitude.

The optimization path for SurrealDB is clear:

1. Add secondary indexes inside `SurrealdbGraphStore`: edge by stable id, outgoing by `(from_id, kind)`, incoming by `(to_id, kind)`, edge-property by `(key, value)`, and maybe node-kind/property indexes.
2. Override the same `GraphStore` hot methods SQLite overrides: `edge`, `sample_edge`, `sample_edge_with_property`, `incident_edges`, `paged_edges`, `paged_incident_edges`, `edges_between_nodes`, and probably `shortest_path_with_max_hops`.
3. Avoid cloning/sorting the full edge set for every probe; return already ordered ranges from the secondary indexes or maintain order-specific maps.
4. Batch projection writes. `replace_projection_rows()` currently clears and then calls `upsert_node` / `upsert_edge` per row, each crossing the Tokio/SurrealDB boundary. A production adapter should bulk load or transact rows and rebuild secondary indexes in one pass.
5. Decide whether SurrealDB itself should own these indexes with query pushdown, or whether the adapter should keep Rust-side derived indexes and treat SurrealKV as durable row storage. The latter is easier and likely enough to test whether SurrealDB can catch SQLite on the current benchmark.

Even after that, I would not assume SurrealDB wins. SQLite has a very favorable profile here: embedded, bundled, low overhead, mature B-tree indexes, stable query plans, WAL semantics, and no async/runtime boundary in the hot read path. SurrealDB may catch up on some graph-shaped workloads if we remove the full scans, but it still has to beat a highly tuned embedded SQLite implementation on every required dataset, including full projection.

So the pragmatic answer is: `yes, SurrealDB is optimizable, and the current adapter has obvious low-hanging fruit; no, we should not treat current parity as performance parity until it gets the same indexed/pushdown treatment SQLite already has and then passes repeated backend-eval samples.`

Commands/evidence used: `agent-doc preflight`, `agent-doc plan`, `agent-doc read`, `tsift --envelope search`, `rg`, and source inspection of `packages/tsift-surrealdb/src/lib.rs`, `packages/tsift-core/src/lib.rs`, `packages/tsift-sqlite/src/lib.rs`, `packages/tsift-cli/src/lib.rs`, and `SPEC.md`. No repo code files were changed.
❯ What are #next-steps to optimize SurrealDB performance?

### Re: SurrealDB performance next steps — gpt-5

Top backlog item: `#surrealidx`.

Priority order:

1. `#surrealidx` — optimize hot reads first. Add secondary indexes to `SurrealdbGraphStore` and override the same high-impact `GraphStore` methods SQLite already optimizes: `edge`, `sample_edge`, `sample_edge_with_property`, `incident_edges`, `paged_edges`, `paged_incident_edges`, `edges_between_nodes`, and bounded path reads. This is the highest leverage because current full-projection losses are dominated by edge lookup, edge property scan, incident scan, related, and path probes paying scan-heavy costs.
2. `#surrealbulk` — optimize projection load second. Replace the per-row `upsert_node` / `upsert_edge` loop in `replace_projection_rows()` with bulk or transactional loading, then rebuild derived indexes once. This separates “SurrealDB load cost” from “adapter loop overhead.”
3. `#surrealpush` — decide the adapter shape. Compare native SurrealDB query pushdown/indexes against Rust-side derived indexes backed by SurrealKV durability. The Rust-side index path is likely the fastest way to test whether SurrealDB can catch SQLite on the current benchmark; native query pushdown is cleaner only if it is fast enough.
4. `#surrealbench` — remeasure with promotion-grade evidence. After the above changes, run repeated `backend-eval --full-projection` samples through `metric-digest`, not one-off samples. SurrealDB should stay on hold unless it beats SQLite across real, full-projection, high-degree, and deep-chain workloads while preserving parity, install portability, and lock behavior.

I added those four items to the backlog in that order. No repo code files were changed.

### Re: #surrealidx — gpt-5

> **Queue prompt:**
>
> [#surrealidx]


Completed `#surrealidx`.

Implemented Rust-side secondary edge indexes in `SurrealdbGraphStore` for stable edge id lookup, ordered edge scans, edge-kind scans, outgoing adjacency, incoming adjacency, and edge-property probes. The SurrealDB adapter now overrides the hot `GraphStore` reads that were falling back to full-edge scans: `edge`, `sample_edge`, `sample_edge_with_property`, `incident_edges`, `paged_edges`, `paged_incident_edges`, `edges_between_nodes`, and bounded path reads. The indexes rebuild on open/load and stay in sync across upsert, edge delete, node delete, and clear.

Changed paths:

- `src/tsift/packages/tsift-surrealdb/src/lib.rs`
- `src/tsift/SPEC.md`

Commits pushed:

- `src/tsift`: `9336a14 feat(graph): index surrealdb hot reads` pushed to `btakita/tsift main`
- `agent-loop`: `73622ef97 chore: update tsift submodule` pushed to `btakita/agent-loop main`

Verification:

- `cargo test --manifest-path packages/tsift-surrealdb/Cargo.toml` passed
- `make check` passed
- `cargo install --path .` passed; installed binary reports `tsift 0.1.62`
- `cargo test -p tsift-cli --features backend-surrealdb cli_parses_graph_db_backend_eval_surrealdb_candidate` passed
- Latest GitHub Actions `CI` run checked: `26784235057` on `main`, conclusion `success`; jobs `clippy + test` and `live Convex graph acceptance` both succeeded. This repo lists `CI` and `Release` workflows; no separate tmux-test workflow is present.

No escalation was used. Confidence: high for the adapter hot-read optimization and parity-preserving behavior covered by tests. I did not run repeated `backend-eval --full-projection` samples here because that is the queued `#surrealbench` item; the next queue item is `#surrealbulk`.
Implemented `#surrealbulk` in the working tree, but I could not complete the required commit/push or agent-doc committed closeout.

> **Queue prompt:**
>
> [#surrealpush]


What changed in `src/tsift`:
- [SPEC.md](/home/brian/work/btakita/agent-loop/src/tsift/SPEC.md)
- [lib.rs](/home/brian/work/btakita/agent-loop/src/tsift/packages/tsift-surrealdb/src/lib.rs)

The SurrealDB adapter now bulk-replaces projection rows in one transaction, preserves deterministic record IDs, and rebuilds Rust-side node/edge indexes in one pass instead of replaying per-row `upsert_node` / `upsert_edge`.

Verification passed:
- `cargo test --manifest-path packages/tsift-surrealdb/Cargo.toml`
- `cargo test -p tsift-cli --features backend-surrealdb graph_db_backend_eval_benchmarks_candidate_stores_against_sqlite -- --nocapture`
- `git diff --check`
- `make check`
- `cargo build --locked`
- `cargo install --path . --locked --force`
- `tsift --version` -> `tsift 0.1.62`
- feature-backed bounded `backend-eval --candidate surrealdb` passed parity on real/high-degree/deep-chain datasets

Blocked:
- `.git` is mounted read-only: `/home/brian/work/btakita/agent-loop/.git type ext4 (ro,...)`
- `git add SPEC.md packages/tsift-surrealdb/src/lib.rs` fails creating `.git/modules/src/tsift/index.lock`
- `agent-doc finalize` wrote the response into `tasks/software/tsift.md` and gated `#surrealbulk`, but failed at the Git commit boundary for the same read-only `.git` reason

So the code is complete and locally verified, but the repo and session document are intentionally left uncommitted until writable Git metadata is restored.

### Re: #surrealbulk — gpt-5

> **Queue prompt:**
>
> [#surrealbulk]


Implemented `#surrealbulk` in the working tree, but commit/push is blocked by the current filesystem mount: `/home/brian/work/btakita/agent-loop/.git` is mounted read-only, so Git cannot create `.git/modules/src/tsift/index.lock` and cannot stage the tsift submodule changes. I am leaving the backlog item gated rather than marking it done until the commit/push boundary can complete.

Changed paths in `src/tsift` working tree:

- `SPEC.md`
- `packages/tsift-surrealdb/src/lib.rs`

What changed: `SurrealdbGraphStore::replace_projection_rows()` no longer clears and replays public `upsert_node` / `upsert_edge` one row at a time. It now converts provider-neutral projection rows once, bulk-replaces SurrealDB node and edge records in one transaction with deterministic record ids, and rebuilds the Rust-side node/edge indexes in one pass from the same rows. The spec now documents the SurrealDB projection-load path and notes that backend-eval reports this load as the candidate `refresh` operation.

Verification passed:

- `cargo test --manifest-path packages/tsift-surrealdb/Cargo.toml`
- `cargo test -p tsift-cli --features backend-surrealdb graph_db_backend_eval_benchmarks_candidate_stores_against_sqlite -- --nocapture`
- `git diff --check`
- `make check`
- `cargo build --locked`
- `cargo install --path . --locked --force`
- `tsift --version` reports `tsift 0.1.62`
- Feature-backed bounded backend-eval sample with `--candidate surrealdb` passed parity for `real`, `synthetic_high_degree`, and `synthetic_deep_chain`; SurrealDB refresh/load timings were `2,170,067us / 14,264 rows`, `150,745us / 1,089 rows`, and `183,178us / 1,280 rows` respectively.

CI review: latest remote `tsift` CI run `26791613768` for the prior `#surrealidx` commit has `clippy + test` passing. Its `live Convex graph acceptance` job failed before the acceptance step started; the failing cache step has no error log, so I classified it as an external CI/cache-start failure rather than a code regression.

Blocked commands:

- `git add SPEC.md packages/tsift-surrealdb/src/lib.rs` failed with `fatal: Unable to create '/home/brian/work/btakita/agent-loop/.git/modules/src/tsift/index.lock': Read-only file system`.
- A direct write probe under `.git/modules/src/tsift` also failed with `Read-only file system`.

Escalation: none available in this harness because approval policy is `never`. Confidence is high for the code change and local verification, but `#surrealbulk` remains gated on restoring writable `.git` access so the intended tsift commit, superproject submodule commit, push, and post-push CI watch can run.
<!-- agent:boundary:3d5b5b86 -->
<!-- /agent:exchange -->
###
<!--
What are #next-steps to promote SurrealDB to SQLite-equivalent substrate status?
-->

## Queue

<!-- agent:queue auto -->
preset #spec-test-build-install-commit-push
- ~[#surrealidx]~
- ~[#surrealbulk]~
- ~[#surrealpush]~
- [#surrealbench]
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog -->
- [ ] [#surrealpush] Compare SurrealDB query pushdown vs Rust-derived indexes behind backend-surrealdb and keep the faster design that preserves GraphStore parity.
- [ ] [#surrealbench] Run repeated backend-eval --full-projection samples through metric-digest after SurrealDB optimizations and keep promotion on hold unless it beats SQLite across required workloads.
<!-- /agent:backlog -->

## Review

<!-- agent:review -->
- [/] [#surrealbulk] Batch SurrealDB projection load: replace per-row upsert loops with bulk/transactional row load plus one-pass secondary-index rebuild; measure refresh/load separately.
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
