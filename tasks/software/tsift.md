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
queue_active: false
---

## Status

<!-- agent:status patch=replace -->

Completed: #gdbpostbase, #gdbrowstream, #gdbprepstage, and #gdbenginegate shipped in tsift c0ebfd5 and superproject 851d29aad; #gdbvacproof is gated on missing live Convex transport credentials after local dry-run and compact proof.
<!-- /agent:status -->

## Spec

See [src/tsift/SPEC.md](../../src/tsift/SPEC.md) for the AST-Aware Code RAG spec.

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc65443c45f7b49f5e6ace54e3f6bafe4077ade57924422b5eb3baf4dbd23c-20260523-012725.md`*

Compacted content:
- Archived 1 response topic(s): SQLite graph performance next steps
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc6544...
### Re: graph performance benchmark normalization — gpt-5

Closed `#gdbbenchnorm`. The current `src/tsift` submodule already contains the implementation in `d56f96f perf: normalize backend eval metrics`, and the superproject `HEAD` already pins `src/tsift` to that commit. I verified that this covers the requested normalization path: `backend-eval` emits raw graph row counts, per-1k-graph-row normalized duration metrics for backend operations and refresh phases, `performance_gate.minimum_sample_runs = 3`, and a repeated-sample command that streams repeated backend-eval JSON into `metric-digest`.

I did not create a new code commit because `src/tsift` was clean and already even with `origin/main`; duplicating the landed change would only churn the same surfaces. The document closeout is the only mutation for this cycle, and the prior finalize consumed the active queue head for `#gdbbenchnorm`.

Verification run:

- `tsift status --fix` in `src/tsift`: refreshed stale local index; instructions current.
- `cargo test graph_db_backend_eval_benchmarks_candidate_stores_against_sqlite --test graph_db_conformance`: passed.
- `make check` in `src/tsift`: passed clippy, 656 unit tests, 108 exit-code tests, and 31 graph-db conformance tests.
- `cargo install --path .`: installed `tsift 0.1.42` to `/home/brian/.cargo/bin/tsift`.
- `tsift --version`: `tsift 0.1.42`.
- `gh run list --workflow CI --limit 1` in `src/tsift`: latest CI succeeded for `perf: normalize backend eval metrics`, run `26321879660`.
- Root `Agent Doc` workflow run `26322731151` showed `check` and `tmux-ci` were not started because of GitHub billing/spending-limit allocation failure, so I classified that as an external CI-start blocker rather than a tmux regression.

Repeated metric evidence from a bounded real three-sample `backend-eval` run with the `kuzu` candidate and selected normalized metrics: loaded 5 runs including the checked-in baseline plus the 3 current samples. Current versus previous normalized results included `real.sqlite.total_duration_micros_per_1k_graph_rows` improving from `31115.48` to `21860.54`, `real.refresh_phase.source_graph_build.duration_micros_per_1k_graph_rows` improving from `15135.53` to `4274.04`, and evidence/synthetic-path metrics remaining noisy enough that they should be judged through repeated samples rather than a single run. Existing `#gdbevidlat` already tracks the follow-up evidence-latency measurement.

Changed paths: none in `src/tsift`; session document only via queue-consumption plus this missed-response repair. Escalation: none. Confidence: high for closing `#gdbbenchnorm`; the implementation, spec, fixture, tests, install, and CI all line up with the requested normalized/repeated-sample performance gate.
### Re: queue closeout repair — gpt-5

Repair-only closeout: the `#gdbbenchnorm` response above is the substantive result. This write syncs the already-consumed queue state while leaving the next queue item open.
❯ #commit-push
### Re: evidence resolver latency — gpt-5

Closed `#gdbevidlat`. I kept the indexed SQLite evidence target resolution path, split backend-eval so the `evidence` operation measures evidence packet collection instead of full conflict-matrix graph snapshot materialization, and added a first-class `evidence_target_resolution` operation plus normalized metrics for real, synthetic high-degree, and synthetic deep-chain datasets.

The repeated real/synthetic measurement used three `backend-eval` samples against the live `#gdbevidlat` target with the `kuzu` candidate. The current `metric-digest` comparison for `real.sqlite.evidence.duration_micros_per_1k_graph_rows` improved from the fixture baseline `26.5449` to `1.3564`, a `94.89%` improvement. The new resolver-specific real samples were `346us`, `27824us`, and `357us` raw, averaging `9509us`; normalized average was `10.148us/1k graph rows`. Synthetic resolver normalized averages were `166.82us/1k` for high-degree and `336.81us/1k` for deep-chain.

Changed paths: `src/tsift/SPEC.md`, `src/tsift/src/main.rs`, `src/tsift/tests/graph_db_conformance.rs`, plus the parent `src/tsift` submodule pointer. The session document is updated by this finalize closeout only.

Verification run: `tsift status --fix`; focused `cargo test graph_db_backend_eval_benchmarks_candidate_stores_against_sqlite --test graph_db_conformance`; three-sample backend-eval plus `metric-digest`; `make check`; `cargo build`; `cargo install --path .`; `tsift --version` reported `tsift 0.1.42`. `make check` passed Clippy, 656 unit tests, 108 exit-code tests, and 31 graph-db conformance tests.

CI/tmux evidence: pushed `tsift` commit `27231cb` (`perf: isolate evidence target timing`) to `origin/main`; GitHub Actions CI run `26323503848` passed both `clippy + test` and `live Convex graph acceptance`. The latest root `Agent Doc` workflow run `26323294795` still has empty-step failed `tmux-ci` and `check` jobs, so I classified it as the known external runner-start blocker rather than a code/tmux regression.

Commit/push: `src/tsift` commit `27231cb` is pushed. Parent repo commit `03e9f860b` (`chore: update tsift evidence timing`) is committed locally and will be pushed after this binary-owned document closeout so the session document commit stays on the finalize path.

Escalation: none. Confidence: high; the benchmark now isolates resolver timing, repeated evidence latency is better than the fixture baseline, the full local suite passed, and the pushed `tsift` CI is green.
❯ So kuzu is ~94% faster than sqlite?
### Re: Kuzu versus SQLite speed — gpt-5

No. The `94.89%` number was not Kuzu versus SQLite. It was the current SQLite `real.sqlite.evidence.duration_micros_per_1k_graph_rows` sample (`1.3564`) compared against the older SQLite fixture baseline (`26.5449`) after the benchmark was fixed to stop charging full conflict-matrix graph snapshot materialization to the evidence packet metric.

In the three current samples, Kuzu did beat SQLite on the isolated evidence metrics, but that is a different comparison: average real evidence packet collection was SQLite `7.50us/1k graph rows` versus Kuzu `1.04us/1k`, while average target resolution was SQLite `10.15us/1k` versus Kuzu `0.094us/1k`. Those averages are skewed by one SQLite outlier (`20.07us/1k` evidence and `29.69us/1k` resolver); the non-outlier samples are much closer.

So the careful reading is: SQLite evidence got about `94.89%` faster versus its old baseline, and the current Kuzu prototype is faster on these isolated evidence/resolver samples, but the `94.89%` claim is not a Kuzu-over-SQLite claim. Kuzu still remains a read-only prototype gate, not a replacement candidate, until a real engine adapter proves parity, install portability, and concurrent-writer behavior.

Changed paths: none. Commands: `agent-doc preflight`, `agent-doc read`, `agent-doc plan`, and `jq` over `/tmp/tsift-gdbevidlat.ndjson` to compare current SQLite/Kuzu samples. Verification: no code changed, so no test suite was run this cycle. Escalation: none. Confidence: high; the clarification comes directly from the saved three-sample backend-eval data and the prior metric-digest comparison.
<!-- agent:boundary:4b63942b -->
<!-- /agent:exchange -->
###
<!--
-->

<!-- agent:queue -->
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog -->
- [/] [#orchdog] [recommended] Dogfood the completed graph-backed agent orchestration stack on a real agent-doc queue run: run refresh -> evidence -> conflict-matrix -> dispatch-trace -> jobs create/collect, compare worker_prompt_packet and worker_result closure fields, and capture any tsift-side fixture gaps. **Dogfood performed 2026-05-21 on `session-share/tasks/root.md` items #x63e + #v4v0; findings recorded in #gdbevid, #cmgr, #gdbidx — see `session-share/tasks/root.md` exchange "graph-backed orchestration dogfood" entry for the full pipeline trace.** **Second dogfood 2026-05-21 against `tasks/software/tsift.md` (preset `#dogfood-tsift`) surfaced 2 new gaps: #cmgrwres and #gdblock.**
<!-- /agent:backlog -->

## Review

<!-- agent:review -->
- [/] [#gdbvacproof] [recommended] Run the real Convex reconciliation plus graph-db compact workflow and capture before/after file size, freelist, tombstone counts, status/doctor timings, and scan-cost deltas so compaction policy is backed by operational evidence.
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
