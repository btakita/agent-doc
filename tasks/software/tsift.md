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

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc65443c45f7b49f5e6ace54e3f6bafe4077ade57924422b5eb3baf4dbd23c-20260524-141331.md`*

Compacted content:
- Archived 3 response topic(s): sqlite graph performance next steps; inactive queue residue bug; graph performance backlog next steps
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc6544...
### Re: backend eval gates — gpt-5

Completed `#gdbcoldfull`, `#gdbprepwm`, `#gdbhopguard`, and `#gdbbackendgate`. The planner scoped this cycle to those four newly added queue targets.

Changed paths:
- `src/tsift/src/main.rs`
- `src/tsift/tests/graph_db_conformance.rs`
- `src/tsift/SPEC.md`
- `src/tsift/README.md`

What changed:
- Added a source-watermark full-projection cache under `.tsift/backend-eval-cache`, with explicit `full_projection.cache_lookup`, `full_projection.source_graph_build`, and `full_projection.projection_rows` timings.
- Fixed generated backend-eval digest commands to use the valid CLI order: `tsift graph-db --path <root> --json backend-eval ...`.
- Fixed conflict-matrix preparation so repo-directory paths can be watermarked instead of being read as markdown files; this unblocks generated backend-eval commands that use `--path <repo-root>`.
- Added full-projection-specific metrics and required performance-gate entries, and made `metric_digest_command` / `repeated_sample_command` include `--full-projection` when that dataset is enabled.
- Kept hop-cap increases behind benchmark-only guardrails: 64 remains the user-facing default, while 128/256/512 stay measured tiers with query-plan checks.
- Put Kuzu behind a native-adapter promotion hold like FalkorDB: native projection load, SQLite parity, full-projection wins, lock behavior, and install portability are required before backend promotion.
- Updated README/SPEC command examples and backend-gate wording to match the executable CLI contract.

Verification:
- `cargo test graph_db_backend_eval_benchmarks_candidate_stores_against_sqlite --test graph_db_conformance -- --nocapture`
- `cargo test conflict_matrix_cli_composes_planner_evidence_and_worker_ownership --test graph_db_conformance -- --nocapture`
- `make check`
- `cargo install --path .`
- `for sample in 1 2 3; do tsift graph-db --path . --json backend-eval --full-projection; done | tsift metric-digest --baseline fixtures/graph-db-performance-history.json --json`

The three-sample full-projection metric digest succeeded (`runs_loaded=12`). It reported the full-projection SQLite total and refresh timings among the largest improvements; the remaining top regressions were in `conflict_matrix_preparation` timing rows.

CI:
- `btakita/tsift` post-push CI run `26363974745` passed: `clippy + test` and `live Convex graph acceptance` both succeeded.
- The root `agent-loop` repo has no workflow named `CI`.
- There is no `tmux-test` leg in the `tsift` CI workflow; the inspected jobs were `clippy + test` and `live Convex graph acceptance`.
- CI annotations are advisory only: Node.js 20 action deprecation and the existing live Convex deployment configuration note.

Commits pushed:
- `btakita/tsift@9520c5c` (`feat: harden backend eval gates`)
- `btakita/agent-loop@2bba5304c` (`chore: update tsift backend eval gates`)

Confidence: high. Escalation: none. Preflight still reported one legacy gated backlog item living in `agent:backlog`; I left that migration alone because it was outside this queue batch.
<!-- agent:boundary:88599535 -->
<!-- /agent:exchange -->
###
<!--
-->

<!-- agent:queue -->
preset #spec-test-build-install-commit-push
- do [#gdbpropscan]
- do [#gdbcoldfull]
- do [#gdbprepwm]
- do [#gdbhopguard]
- do [#gdbbackendgate]
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog -->
- [ ] [#gdbpropscan] [recommended] Isolate the slow SQLite edge_property_scan path from #gdbedgescan: reproduce the latest roughly 3s fixture, capture EXPLAIN plus row-count/cardinality evidence, then prove any materialized edge-property index or scan-shape change with backend-eval and metric-digest before broader backend work.
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
