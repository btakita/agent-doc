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
<!-- agent:boundary:acb2b566 -->
<!-- /agent:exchange -->
###
<!--
-->

<!-- agent:queue auto -->
dispatch #spec-test-build-install-commit-push
- ~do [#gdbsourcebuild]~
- ~do [#gdbprepreuse]~
- do [#gdbbenchnorm]
- do [#gdbevidlat]
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog -->
- [ ] [#gdbevidlat] [recommended] Re-measure the indexed graph-db evidence resolver with repeated real and synthetic runs, because the last benchmark showed evidence target resolution moving from about 24.9ms to 36.5ms; keep the indexed lookup path only if repeated metric-digest output proves it is neutral or faster under the current graph size.
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
