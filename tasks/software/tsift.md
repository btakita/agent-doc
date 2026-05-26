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
  '#next-steps': Any follow-up items to place in the backlog? Please ensure the backlog is in priority order. Print the item id of the top of the backlog.
  '#spec-test-build-install-commit-push': update spec + tests. build + install for local testing. commit + push
  '#spec-test-commit-push': update spec + tests. commit + push
queue_active: true
---

## Status

<!-- agent:status patch=replace -->
Completed: #gfront recorded fresh full-projection path/evidence metrics and tightened the SQLite frontier guard in tsift 3012a32 and agent-loop 1543ea55f. Local verification passed (`cargo test -q --test scan_plan`, backend-eval contract test, `make check`, `cargo build`, `cargo install --path .`); latest pre-push tsift CI run 26431208328 is green, while root Agent Doc CI still shows empty-step runner-start failures.
<!-- /agent:status -->

## Spec

See [src/tsift/SPEC.md](../../src/tsift/SPEC.md) for the AST-Aware Code RAG spec.

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc65443c45f7b49f5e6ace54e3f6bafe4077ade57924422b5eb3baf4dbd23c-20260526-030403.md`*

Compacted content:
- Archived 1 response topic(s): #next-steps graph performance
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc6544...
### Re: #gperf graph performance — gpt-5

Completed #gperf. I ran three fresh `tsift graph-db --path /home/brian/work/btakita/agent-loop --json backend-eval --full-projection` samples covering the real, full-projection, high-degree, and deep-chain workloads, then digested them against `fixtures/graph-db-performance-history.json`.

Tracked updates:

- `src/tsift/fixtures/graph-db-performance-history.json`: appended three `agent-loop-gperf-full-projection-2026-05-26-sample-{1,2,3}` runs.
- `src/tsift/plans/gperf-evidence.md`: recorded the sample table, metric-digest result, hotspot ranking, and verdict.
- `src/tsift/SPEC.md`: documented the additive fixture metadata fields already used by the performance history.

Main result: all three full-projection cache checks missed (`cache.hit=0`) and pruned one sibling cache file. A control run written outside the repo also missed, so this is not just report-output pollution. The largest current hotspots are `full_projection.source_graph_build` (median 24,103,353 us), `full_projection.sqlite.replace_projection_total` (18,703,736 us), and `full_projection.sqlite.sqlite_delta_write` (11,153,707 us). The high-hop path probes are not the bottleneck; `synthetic_deep_chain.sqlite.path_max_hops_512` had a 566 us median.

Verdict: do not raise user-facing max-hop defaults and do not promote a prototype backend from this evidence. I captured a follow-up backlog item for the source-watermark/full-projection cache reuse finding before revisiting caps or backend promotion.

Commits prepared locally before this document closeout:

- `src/tsift`: `c48a073` (`docs(graph-db): record gperf performance triage`)
- `agent-loop`: `de6e2381` (`chore(tsift): pin gperf performance triage`)

Verification:

- `make check` in `src/tsift`: passed (`cargo clippy --all-targets -- -D warnings`, `cargo test`: 687 unit tests, 124 exit-code tests, 33 graph-db conformance tests, 21 perf-gate tests, 4 scan-plan tests).
- `cargo build` in `src/tsift`: passed.
- `cargo install --path .` in `src/tsift`: passed; installed `tsift 0.1.62`.
- GitHub Actions: latest tsift `CI` run `26418433229` is green.
- No `tmux-test` workflow is present in the tsift repo; root agent-loop has only the `Agent Doc` workflow, whose latest failures are separate 3-4 second session-document jobs, not a tsift test leg.

Per the agent-doc commit boundary, I will push the tsift commit, the agent-loop pointer commit, and this session-document commit after finalize succeeds.
### Re: #ghop hop-cap promotion gate — gpt-5

Completed #ghop. The user-facing graph path default remains 64 hops until `perf_gate::evaluate_hop_cap_promotion` promotes a measured candidate tier.

Implemented:

- `src/tsift/src/perf_gate.rs`: added hop-cap constants, `HopCapGateReport`, workload verdicts, and evaluator. The gate requires 128/256/512 candidates only; repeated SQLite samples for `real`, `full_projection`, and `synthetic_deep_chain`; latency within the allowed regression budget against 64-hop `path_max_hops.duration_micros`; useful rows; and deeper synthetic-chain rows beyond the 64-hop median.
- `src/tsift/src/main.rs`: `graph-db backend-eval` now emits `performance_gate.hop_cap_promotion` with current default, candidate tiers, required workloads/metrics, and decision rule.
- `src/tsift/tests/perf_gate.rs` and `src/tsift/tests/graph_db_conformance.rs`: covered fixture blocking, synthetic pass, missing full-projection proof, deep-chain row usefulness, unmeasured candidate tiers, and backend-eval JSON contract.
- `src/tsift/SPEC.md` and `src/tsift/VERSIONS.md`: documented the hop-cap promotion rule.

Commits prepared locally before this document closeout:

- `src/tsift`: `06a0383` (`feat(graph-db): gate hop-cap promotion`)
- `agent-loop`: `242b99610` (`chore(tsift): pin hop-cap promotion gate`)

Verification:

- `cargo test -q perf_gate`: passed.
- `cargo test -q graph_db_backend_eval_benchmarks_candidate_stores_against_sqlite`: passed.
- `make check` in `src/tsift`: passed (`cargo clippy --all-targets -- -D warnings`, `cargo test`: 692 unit tests, 124 exit-code tests, 33 graph-db conformance tests, 30 perf-gate tests, 4 scan-plan tests).
- `cargo build` in `src/tsift`: passed.
- `cargo install --path .` in `src/tsift`: passed; installed `tsift 0.1.62`.
- GitHub Actions: latest tsift `CI` run `26430632139` passed (`clippy + test`, `live Convex graph acceptance`).
- Tmux/Agent Doc CI: root `Agent Doc` run `26430663631` failed before job startup with empty-step `tmux-ci` and `check` jobs; I treated this as an external CI-start blocker, not a code/tmux regression.

Per the agent-doc commit boundary, I will push the tsift commit, the agent-loop pointer commit, and this session-document commit after finalize succeeds.
### Re: #gfront SQLite frontier guard — gpt-5

Completed #gfront. I took a fresh full-projection backend-eval sample before changing code. SQLite path/evidence reads are not the current hotspot: full-projection SQLite `path_max_hops` was 45 us, `path_max_hops_512` was 26 us, `evidence_target_resolution` was 515 us, and `evidence` was 957 us. Synthetic deep-chain SQLite `path_max_hops_512` was 564 us. The roughly 1.1s path timings are in the prototype read-only stores, not SQLite frontier traversal, so I did not rewrite SQLite traversal without a measured SQLite hotspot.

Implemented the guard instead:

- `src/tsift/plans/gfront-evidence.md`: recorded the fresh real/full-projection/high-degree/deep-chain path and evidence metrics plus the decision not to rewrite SQLite frontier traversal.
- `src/tsift/src/main.rs`: full-projection performance gate now requires SQLite evidence-target, evidence, and 64/128/256/512-hop path duration metrics.
- `src/tsift/tests/scan_plan.rs`: added chunked frontier coverage proving multi-node frontier probes use `idx_graph_edges_from_kind`, extending the existing single-node frontier plan test.
- `src/tsift/tests/graph_db_conformance.rs`: locks the new full-projection required metric contract.
- `src/tsift/SPEC.md` and `src/tsift/VERSIONS.md`: documented the SQLite frontier guard and evidence boundary.

Commits prepared locally before this document closeout:

- `src/tsift`: `3012a32` (`test(graph-db): guard sqlite frontier metrics`)
- `agent-loop`: `1543ea55f` (`chore(tsift): pin sqlite frontier guard`)

Verification:

- Fresh metric sample: `tsift graph-db --path /home/brian/work/btakita/agent-loop --json backend-eval --full-projection | jq ...` passed after known Kotlin grammar warnings.
- `cargo test -q --test scan_plan`: passed (5 tests).
- `cargo test -q graph_db_backend_eval_benchmarks_candidate_stores_against_sqlite`: passed.
- `make check` in `src/tsift`: passed (`cargo clippy --all-targets -- -D warnings`, `cargo test`: 692 unit tests, 124 exit-code tests, 33 graph-db conformance tests, 30 perf-gate tests, 5 scan-plan tests).
- `cargo build` in `src/tsift`: passed.
- `cargo install --path .` in `src/tsift`: passed; installed `tsift 0.1.62`.
- GitHub Actions before this push: tsift `CI` run `26431208328` passed (`clippy + test`, `live Convex graph acceptance`).
- Tmux/Agent Doc CI before this push: root `Agent Doc` run `26431210323` failed before job startup with empty-step `tmux-ci` and `check` jobs; I treated this as an external CI-start blocker.

Per the agent-doc commit boundary, I will push the tsift commit, the agent-loop pointer commit, and this session-document commit after finalize succeeds.
<!-- agent:boundary:6e1601b0 -->
<!-- /agent:exchange -->
###
<!--
What are #next-steps to make the graph faster?
-->

<!-- agent:queue auto -->
preset #spec-test-build-install-commit-push
- ~do [#gperf]~
- ~do [#ghop]~
- ~do [#gfront]~
- do [#gback]
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog -->
- [ ] [#gcachemiss] Stabilize full-projection source-watermark/cache reuse: reproduce repeated #gperf cache misses on a quiescent checkout, identify which watermark input shifts between runs, and gate cache-hit evidence before cap/backend changes.
- [ ] [#gback] [recommended] Spike a real optional graph backend adapter only after SQLite gate evidence is fresh: compare Kuzu and FalkorDB behind GraphStore with projection writes/load, lock semantics, install portability, and full parity; do not promote a read-only prototype unless it beats SQLite across every required workload.
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
