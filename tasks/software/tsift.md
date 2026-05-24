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
Completed: #gdbrefreshcache, #gdbtargetsnap, and #gdbpathprobe shipped in tsift c1c7668 and superproject f321eabef; GitHub Agent Doc CI is externally blocked before runner startup by the billing/spending-limit annotation.
<!-- /agent:status -->

## Spec

See [src/tsift/SPEC.md](../../src/tsift/SPEC.md) for the AST-Aware Code RAG spec.

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc65443c45f7b49f5e6ace54e3f6bafe4077ade57924422b5eb3baf4dbd23c-20260524-130428.md`*

Compacted content:
- Archived 1 response topic(s): sqlite graph performance next steps
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc6544...
### Re: graph-db backend gates — gpt-5

Completed the four queued tsift items.

Changed paths:
- `src/tsift/SPEC.md`
- `src/tsift/fixtures/graph-db-performance-history.json`
- `src/tsift/src/main.rs`
- `src/tsift/src/substrate.rs`
- `src/tsift/tests/graph_db_conformance.rs`

What changed:
- `#gdbrefreshfast`: graph-db refresh JSON now distinguishes `cold_source_graph_rebuild` from `cached_source_watermark_reuse`, with conformance coverage proving the source-watermark reuse path.
- `#gdbedgescan`: SQLite incident scans now use UNIONed `from_id`/`to_id` index probes instead of the old OR predicate; tests compare UNION results against OR semantics and assert the edge lookup, incident, and edge-property scan plans use the intended indexes.
- `#gdbhopcaps`: backend-eval keeps the user-facing 64-hop default but adds measured 128/256/512-hop tiers on a larger synthetic deep-chain fixture, with metrics and performance-gate fixture coverage for the 512-hop tier.
- `#gdbfalkorgate`: backend-eval promotion decisions now carry an explicit native FalkorDB gate requiring native projection load, freshness/parity, lock behavior, and install-cost proof before any performance-win claim.

Verification commands:
- `cargo test --test graph_db_conformance graph_db_backend_eval_benchmarks_candidate_stores_against_sqlite -- --nocapture`
- `cargo test --test graph_db_conformance graph_db_scale_caps_pagination_paths_doctor_and_sqlite_plans -- --nocapture`
- `cargo test --test graph_db_conformance graph_db_refresh_and_status_materialize_operator_report -- --nocapture`
- `make check`
- `git diff --check`
- `cargo install --path .`
- `tsift --version` -> `tsift 0.1.42`
- `gh run view 26362394169 --json status,conclusion,jobs` -> `completed`, `success`; jobs `clippy + test` and `live Convex graph acceptance` both passed.

CI and tmux evidence:
- tsift CI run `26362394169` passed after the push.
- Superproject Agent Doc CI run `26362404104` failed before runner steps were allocated: both `check` and `tmux-ci` jobs had empty `steps`. I’m recording that as the known external CI-start blocker rather than a code or tmux regression.

Commits and push:
- tsift: `d4b1405 feat: harden graph db backend gates` pushed to `btakita/tsift/main`.
- superproject: `788b47be2 chore: update tsift graph db gates` pushed to `btakita/agent-loop/main`.
- Note: the superproject branch head after push is `d13d3d13e`; my superproject change in that push is `788b47be2` and only updates the `src/tsift` submodule pointer.

Escalation: none.
Confidence: high.
❯ What are the performance differences from the last queue run?
### Re: performance differences from last queue run — gpt-5

I compared the previous queue baseline in tsift commit `c1c7668` (`real-agent-loop-batched-projection-2026-05-22`) with the latest fixture record in `d4b1405` (`real-agent-loop-2026-05-23-falkor-full-projection`). This is a useful comparison, but not a perfect apples-to-apples benchmark: the latest record separates full-projection cost and exercises FalkorDB/full-projection paths, while the older record was the batched projection queue baseline.

Main deltas:

| Metric | Previous | Latest | Delta |
| --- | ---: | ---: | ---: |
| Real SQLite total | 29.199s | 3.989s | -86.34% |
| Real SQLite refresh | 24.414s | 0.462ms | effectively cache-hit reuse |
| Real SQLite dispatch trace | 1.491s | 5.776ms | -99.61% |
| Real SQLite evidence | 24.910ms | 1.484ms | -94.04% |
| Real SQLite 64-hop path | 1.029s | 0.145ms | -99.99% |
| Real FalkorDB total | 6.170s | 24.157s | +291.51% |
| Real FalkorDB refresh | 3.238s | 5.546s | +71.30% |
| Synthetic deep-chain SQLite total | 2.930ms | 20.709ms | +606.79% |
| Synthetic high-degree SQLite refresh | 25.338ms | 21.320ms | -15.86% |
| Synthetic high-degree SQLite total | 27.776ms | 33.485ms | +20.55% |

Interpretation:

- The big SQLite real-run win is the source-watermark/cache reuse path: `real.sqlite.refresh.duration_micros` drops from `24,413,795` to `462`. The latest run also records full-projection cost separately, so cold full rebuild is still expensive: `full_projection.source_graph_build` is `58.790s` and `full_projection.projection_rows` is `19.745s`.
- The hot SQLite query probes are much better in the latest record: dispatch trace, evidence, and 64-hop path all moved from millisecond/second-scale timings into microsecond-to-low-millisecond timings.
- The latest run adds new metrics that did not exist in the prior queue baseline: real SQLite `edge_lookup` is `291us`, `incident_edges` is `330us`, and `edge_property_scan` is `3.069s`. That last one is now the standout expensive indexed lookup path.
- The max-hop work changed the synthetic deep-chain fixture shape from `384` graph rows to `640`, so the total slowdown there is expected workload expansion, not a regression against the same dataset. The new SQLite hop timings are still bounded: 128-hop `467us`, 256-hop `890us`, 512-hop `1.650ms`.
- The FalkorDB numbers argue against promotion without the native adapter. In the latest full-projection record, FalkorDB is slower than the previous bounded/simulated path and far slower than cached SQLite for real-run total time; that matches the new promotion gate.

Changed paths: none for this response.
Commands used: `agent-doc preflight`, `agent-doc read`, `agent-doc plan`, `jq` comparisons over `fixtures/graph-db-performance-history.json`, and `git show c1c7668:fixtures/graph-db-performance-history.json`.
Verification: no code changed; this was a fixture/history comparison only.
Escalation: none.
Confidence: medium-high, because the comparison is accurate for the recorded fixture values but the two runs are not identical workload shapes.
<!-- agent:boundary:1d13f800 -->
<!-- /agent:exchange -->
###
<!--
-->

<!-- agent:queue -->
preset #spec-test-build-install-commit-push
- do [#gdbrefreshfast]
- do [#gdbedgescan]
- do [#gdbhopcaps]
- do [#gdbfalkorgate]
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
