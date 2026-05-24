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
### Re: `do [#gdbprephotreuse]` — session_discovery -96% via mtime cap + cwd header gate — opus-4-7

Three-sample medians on agent-loop's real `tsift graph-db --json backend-eval` proved that **`session_discovery` was 96% of `session_review_compute`** under the new 0.1.43 sub-phase timers, not the cross-phase packet duplication that the original `#gdbprephotreuse` plan assumed. Fix landed in **tsift 0.1.44 (commit 052360d)** with agent-loop submodule pointer bumped to **e14949dfd**. The parent `#gdbprephot` stays gated in review — this cycle closes only `#gdbprephotreuse`.

#### Sample evidence (3 cold runs each)

| phase (median ms) | pre (0.1.43) | +cwd header gate only | +mtime cap (final) |
|---|---:|---:|---:|
| `conflict_matrix_preparation` | 5887.8 | 5500.9 | **2147.8 (-64%)** |
| `…session_review_compute` | 3718.9 | 3449.6 | **271.6 (-93%)** |
| `…session_review_compute.session_discovery` | 3562.4 | 3307.2 | **154.3 (-96%)** |
| `…session_review_compute.session_digest_total` | 79.1 | 74.9 | 81.2 |

The header-gate-only attempt was a 7% nibble; the mtime cap is what made the step change. Reason: most JSONL files in `~/.codex/sessions/` and `~/.claude/projects/.../` belong to *some* session under agent-loop (cwd matches), so the cwd gate alone rejected only a small fraction. Stat-sorting + capping at the 64 most-recent files per source skips ~98% of the ~2 GB / 2323-file Codex history and ~1.5K Claude sessions before any content read, while still always covering the `MAX_SESSIONS=12` global truncation.

#### Why this is *not* the original sub-step body from the parent plan

The parent plan called for sharing per-target context/diff packets across `session_review_compute → impact → context_pack_diff`. The new sub-phase data invalidates that premise — those three phases together account for at most ~156 ms of the prior 3.7 s `session_review_compute` cost (digest 79 ms + cost 37 ms + aggregation 55 ms + 0.3 ms assembly). Cross-phase packet reuse would have been a structural change for sub-100 ms savings. Closing this sub-step against the real bottleneck instead of the speculative one.

#### Implementation (src/tsift/src/session_review.rs)

1. New `collect_recent_files_with_extension(dir, extension, limit)` — stat-walks the directory, sorts by mtime desc, returns at most `limit` paths.
2. New `MAX_RECENT_CANDIDATES_PER_SOURCE = 64` (well above `MAX_SESSIONS = 12` so cross-source merge still produces the genuinely most-recent global top-12 even when some files reject during cwd matching).
3. New `read_jsonl_session_text_if_cwd_matches(path, context, label, extract_cwd)` — `BufReader` reads header lines (256 KB budget) and runs `extract_claude_cwd_from_text` / `extract_codex_cwd_from_text` until the cwd is found; skips the full read entirely when `cwd_matches_target` fails.
4. `maybe_add_claude_candidate` and `maybe_add_codex_candidate` now call the header-gated reader and the discovery loops use the bounded walk for Claude and Codex sources (agent-doc logs stay on the unbounded walk — they are small and few).
5. SPEC.md line 47 updated to describe the bounded walk + header gate; VERSIONS.md adds `## 0.1.44` with the measured deltas.

#### Verification

| Check | Result |
|---|---|
| `cargo test --release` (tsift, full suite) | 662 + 108 + 31 = 801 tests pass; conformance fixture now runs in 1.92 s (was 38.28 s) confirming the discovery walk was a hot path |
| New unit tests | `collect_recent_files_with_extension_caps_and_sorts_by_mtime` and `read_jsonl_session_text_if_cwd_matches_skips_non_matching_files_without_full_read` |
| `cargo build --release` + `cargo install --path .` | `tsift --version` prints `tsift 0.1.44` |
| `git push` (tsift `main` → `origin/main`) | `052360d` pushed |
| `git push` (agent-loop `main` → `origin/main`) | `e14949dfd` pushed |
| GitHub Actions CI | run 26375771323 launched at finalize time; local 801-test suite is authoritative for the regression assertions added this cycle |

#### Queue state

Queue head is now `do [#gdbprephotgate]`. The 3-sample baseline data is already in this cycle for its sibling phase `status_index_gate` (not measured per-sub-phase here, just the parent); the next cycle should start by capturing the `conflict_matrix_preparation.status_index_gate` median against the new 0.1.44 numbers before changing the gate path.
<!-- agent:boundary:71e92572 -->
<!-- /agent:exchange -->
###
<!--
What are #next-steps to make the graph faster?
-->

<!-- agent:queue auto -->
preset #spec-test-build-install-commit-push
- ~do [#gdbprephotgate]~
- do [#gdbfullreuse]
- do [#gdbscanproof]
- do [#gdbperfgate]
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog -->
- [ ] [#gdbgatecold] [recommended] Reduce the cold-path cost of conflict_matrix_preparation.status_index_gate. The in-process cache landed in 0.1.45 covers daemon/multi-call use but does not help single-shot CLI invocations because each helper fires only once per backend-eval pipeline. Three-sample medians attribute the 678ms median to prepare_agent_doc_index_gate (~422ms inspect_search_index + apply_search_index_update) and context_pack_status_reminders (~266ms). Investigate whether inspect_search_index can fast-path a freshness verdict from a cheap watermark check (e.g., index db mtime + source root mtime hash) before any DB open, and whether context_pack_status_reminders can read fewer files. Verify with 3-sample medians under conflict_matrix_preparation.status_index_gate.{prepare_agent_doc_index_gate, context_pack_status_reminders} before claiming reduction.
- [ ] [#impactprof] [recommended] After conflict_matrix_preparation.session_review_compute dropped from 3719ms to 272ms (#gdbprephotreuse, tsift 0.1.44) and status_index_gate observability landed (#gdbprephotgate, tsift 0.1.45), conflict_matrix_preparation.impact is now the dominant single sibling at 774-810ms median. Profile impact::compute by adding sub-phase timers (mirror the session_review_compute and status_index_gate decomposition pattern), capture a 3-sample baseline under conflict_matrix_preparation.impact.<sub>, then implement the proven inner-call reduction with a metric-digest regression gate.
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
