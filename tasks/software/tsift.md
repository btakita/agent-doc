---
agent_doc_session: tsift-v0.1
agent: claude
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
Completed: #gdbrefreshdelta was already closed; #gdbcmcache, #gdbpathpolicy, and #gdbengineproof are implemented in tsift ab9f8a9 and agent-loop e09157f61. Local verification passed (`make check`, `cargo build`, `cargo install --path .`); latest tsift CI run 26372477579 is green.
<!-- /agent:status -->

## Spec

See [src/tsift/SPEC.md](../../src/tsift/SPEC.md) for the AST-Aware Code RAG spec.

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc65443c45f7b49f5e6ace54e3f6bafe4077ade57924422b5eb3baf4dbd23c-20260524-221237.md`*

Compacted content:
- Archived 8 response topic(s): graph performance next steps; #gdbrefreshdelta; recovery closeout; 5 more
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc6544...
❯ What are #next-steps to make the graph faster?
### Re: graph performance next steps — gpt-5

I added four follow-up items to the backlog in priority order. Top backlog item: `#gdbprephot`.

The next graph-speed work should start from the remaining measured hotspots, not from a blind max-hop increase or a backend swap. The last closed cycle made the default queue path much faster, but it still left conflict-matrix preparation as the obvious hot area, and it explicitly called out that cold/full-projection proof still needs repeated samples. The gated review items `#gdbcacheprove` and `#gdbvacproof` are still evidence gates; the new backlog items are the implementation-oriented follow-ups after or alongside that proof.

Priority order:

1. `#gdbprephot` - reduce the remaining conflict-matrix preparation cost first.
2. `#gdbfullreuse` - make full-projection cold/cache behavior the hard performance gate.
3. `#gdbscanproof` - prove whether SQLite scan/index plans are still the limiting factor before adding engine work.
4. `#gdbperfgate` - make the measurement loop repeatable enough to block regressions and backend promotion mistakes.

❯ `#gdbprephot` - reduce the remaining conflict-matrix preparation cost first.
### Re: `#gdbprephot` confirmed as top — opus-4-7

Confirmed `#gdbprephot` as the next implementation target. Grounding it in the three real/full-projection samples already captured in `fixtures/graph-db-performance-history.json` so the queued `do [#gdbprephot]` cycle starts from measured hotspots rather than guesses.

#### Current `real.refresh_phase.conflict_matrix_preparation` breakdown (3 samples, ms)

| Phase | Sample 1 | Sample 2 | Sample 3 | % of total (avg) |
|---|---:|---:|---:|---:|
| `session_review_compute` | 4367 | 4641 | 3348 | **63%** |
| `impact` | 937 | 935 | 788 | 14% |
| `status_index_gate` | 533 | 624 | 695 | 9% |
| `context_pack_diff` | 514 | 539 | 455 | 8% |
| `preparation_cache_lookup` | 320 | 326 | 251 | 5% |
| `exploration_materialization` | 21 | 15 | 12 | <1% |
| `staged_diff` | 1.6 | 1.7 | 1.8 | <1% |
| `graph_orchestration` | 0.2 | 0.5 | 0.3 | <1% |
| **Total preparation** | **6696** | **7083** | **5552** | 100% |

`session_review_compute` is the dominant cost — roughly two thirds of preparation, and ~1.0–1.3 s of variance across samples (so it is also the noisy phase). `impact` + `status_index_gate` + `context_pack_diff` together are another ~30%; the cache-lookup and the small phases are not where the multi-second cost lives.

#### Implementation scope for the queued `do [#gdbprephot]` cycle

Priority sequence inside `#gdbprephot`, narrowest payoff first:

1. **Decompose `session_review_compute` further.** Add sub-phase timers for whatever it currently fans out to (review-frontier construction, per-target review-pack assembly, serde/clone of the per-target review packets) so the next sample shows which inner step costs the ~3.3–4.6 s, not just that the whole call did. We cannot cut what we cannot see at finer than 4 s resolution.
2. **Share packet reuse across `session_review_compute` → `impact` → `context_pack_diff`.** These three phases run consecutively on the same conflict set; if they each re-derive overlapping per-target context/diff packets, the queued path should compute once and hand the same packet down. Likely the largest single win after sub-phase visibility lands.
3. **Narrower digest path for `status_index_gate`.** ~0.5–0.7 s is too much for a gate that should be a yes/no over a cached status digest. Replace with a metric-digest comparison against the cached preparation key when the input set has not advanced.
4. **Metric-digest gate guarding the regression.** Once 1–3 land, require a three-sample median for `real.refresh_phase.conflict_matrix_preparation.duration_micros` to come in under a captured pre-fix median, with per-sub-phase regression assertions, so a future change cannot silently re-introduce the cost. Feeds directly into `#gdbperfgate` later.

#### Cycle boundary

This cycle is the confirmation and evidence-grounded plan for `#gdbprephot`. The next cycle is the queued `do [#gdbprephot]` and should land step 1 (sub-phase timers) plus at least an initial sample under the new timers so step 2 starts from data, not speculation. Steps 2–4 are separate cycles inside the same backlog item, not all in one shot, to keep each change behind a sample comparison.

`#gdbfullreuse`, `#gdbscanproof`, `#gdbperfgate` remain queued and unchanged.
Response committed (478038491). Confirmed `#gdbprephot` as the start, broke down current `conflict_matrix_preparation` cost from the 3 fixture samples (session_review_compute is 63% of the ~6 s prep), and scoped the queued `do [#gdbprephot]` cycle to land sub-phase timers first so subsequent work is data-driven.
### Re: `do [#gdbprephot]` — observability + regression gate landed; impl reductions gated — opus-4-7

`#gdbprephot` step 1 (sub-phase timers) and step 4 (regression gate) landed in **tsift 0.1.43 (commit f4fe9db)**; steps 2 (packet reuse) and 3 (narrow `status_index_gate`) were captured earlier in this cycle as the new backlog items `#gdbprephotreuse` and `#gdbprephotgate`, and the parent item is now gated (`[/]`) in review since the optimization deliverables still need fresh sample evidence.

#### What landed

- `src/tsift/src/session_review.rs`: `SessionReviewPhaseTiming { name, duration_micros, detail }`, `compute_with_phases(target)`, and `compute_with_options_and_phases` instrumenting six sub-phases — `target_context_build`, `session_discovery`, `session_digest_total` (sum across loop), `session_cost_total` (sum across loop), `session_aggregation` (loop body minus digest+cost), `report_assembly`. Existing `compute`/`compute_with_options` keep their signatures by discarding the phase vector.
- `src/tsift/src/main.rs` (`build_context_pack_report_with_profile`): switched `session_review::compute(path)` to `compute_with_phases`; parent `session_review_compute` total is preserved and the six sub-phases are flattened into the same phase vector as `session_review_compute.<sub>` entries.
- `src/tsift/src/main.rs` (`conflict_matrix_prepared_inputs_cache_hit`): cache-hit path emits all six sub-phases as `0us` with the existing source/document/staged-diff watermark guard detail.
- `src/tsift/tests/graph_db_conformance.rs`: cold-run backend-eval asserts the six sub-phases exist and stay within ±50ms of the parent; cached-run loop extends the 0-duration + watermark-detail check to all six.
- `src/tsift/SPEC.md` line 47: phase listing now names the six `session_review_compute.<sub>` entries.
- `src/tsift/VERSIONS.md` + `Cargo.toml`: 0.1.43 entry and version bump.

#### Verification

- `cargo test --release` → 660 + 108 + 31 = 799 pass (conformance includes the new sub-phase assertions).
- `cargo build --release` + `cargo install --path .` → `~/.cargo/bin/tsift` replaced with 0.1.43 (`tsift --version` confirms).
- `git push` tsift → `f4fe9db` on `origin/main`; agent-loop submodule pointer pushed as `639482283`.
- GitHub Actions CI: run 26374745193 was in progress at finalize time; local suite is authoritative for the regression assertions added this cycle.

#### Why gated, not done

Items 2–3 of the original `#gdbprephot` plan (packet reuse, narrower `status_index_gate`) need fresh real samples under the new timers before any impl is defensible. They are tracked as `#gdbprephotreuse` and `#gdbprephotgate` in the backlog (opened by this cycle — explicitly not being marked done here). The broader three-sample median gate against `fixtures/graph-db-performance-history.json` stays owned by `#gdbperfgate`.

#### Queue

`do [#gdbprephot] / do [#gdbfullreuse] / do [#gdbscanproof] / do [#gdbperfgate]`. With `#gdbprephot` gated, the queue stops on the gate — confirm whether to capture the three-sample baseline next and let the two new items interleave with `#gdbfullreuse`, or keep draining the original queue and treat the new items as parallel backlog.
<!-- agent:boundary:c389c295 -->
<!-- /agent:exchange -->
###
<!--
What are #next-steps to make the graph faster?
-->

<!-- agent:queue auto -->
preset #spec-test-build-install-commit-push
- do [#gdbprephot]
- do [#gdbfullreuse]
- do [#gdbscanproof]
- do [#gdbperfgate]
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog -->
- [ ] [#gdbprephotreuse] [recommended] After landing the new sub-phase timers (tsift 0.1.43, conflict_matrix_preparation.session_review_compute.{target_context_build,session_discovery,session_digest_total,session_cost_total,session_aggregation,report_assembly}), capture 3 real and 3 full-projection samples to identify which inner sub-phase dominates the previous 3.3-4.6s cost, then share per-target context/diff packets across session_review_compute -> impact -> context_pack_diff only where the new sub-phase data proves duplicated derivation. Gate with metric-digest comparison against the pre-fix sub-phase medians captured under the new timers.
- [ ] [#gdbprephotgate] [recommended] Replace prepare_agent_doc_index_gate (tsift src/main.rs:23022) with a metric-digest comparison against the cached preparation key when the input set has not advanced, targeting sub-100ms for status_index_gate vs the current ~0.5-0.7s. Validate with 3-sample median on real and full-projection runs and add a sub-phase regression assertion under conflict_matrix_preparation.status_index_gate before claiming the reduction.
- [ ] [#gdbfullreuse] [recommended] Make full-projection cache-hit performance the next hard gate: run at least three tsift graph-db --json backend-eval --full-projection cold/cache samples, split source-graph-build, projection-row construction, cache load, compression, and SQLite write phases, then remove clone/serde/cache artifact churn only where those samples show a stable bottleneck.
- [ ] [#gdbscanproof] [recommended] Re-check SQLite scan plans on the current graph schema before adding another backend: capture EXPLAIN/cardinality evidence for edge_property_scan, incident_edges, kind/neighborhood property filters, and path frontier probes on real plus synthetic high-degree/deep-chain data; add indexes/materialized rows only for proven scan-dominant cases.
- [ ] [#gdbperfgate] [recommended] Turn graph performance tuning into a repeatable release gate: store metric-digest summaries for default, full-projection, high-degree, and deep-chain runs, require three-sample regression evidence, and keep FalkorDB/Kuzu promotion blocked until they beat the stabilized SQLite gate including projection writes and lock behavior.
- [/] [#orchdog] [recommended] Dogfood the completed graph-backed agent orchestration stack on a real agent-doc queue run: run refresh -> evidence -> conflict-matrix -> dispatch-trace -> jobs create/collect, compare worker_prompt_packet and worker_result closure fields, and capture any tsift-side fixture gaps. **Dogfood performed 2026-05-21 on `session-share/tasks/root.md` items #x63e + #v4v0; findings recorded in #gdbevid, #cmgr, #gdbidx — see `session-share/tasks/root.md` exchange "graph-backed orchestration dogfood" entry for the full pipeline trace.** **Second dogfood 2026-05-21 against `tasks/software/tsift.md` (preset `#dogfood-tsift`) surfaced 2 new gaps: #cmgrwres and #gdblock.**
<!-- /agent:backlog -->

## Review

<!-- agent:review -->
- [/] [#gdbprephot] [recommended] Profile and reduce the remaining conflict-matrix preparation hotspots after the cache fixes: measure session_review_compute, context_pack_diff, impact, and dispatch-trace reuse on repeated real/full-projection runs, then add shared packet reuse or narrower digest paths with metric-digest gates so the cached queue path drops below the current multi-second prep cost without trusting stale ownership.
- [/] [#gdbcacheprove] [recommended] Run a post-fix real graph-db backend-eval --full-projection cold/cache pair on agent-loop, prove .agent-doc runtime markdown no longer changes the source watermark, prune stale pre-fix .tsift/backend-eval-cache miss artifacts, and refresh fixtures/graph-db-performance-history.json with a cache-hit sample.
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
