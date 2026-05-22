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

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc65443c45f7b49f5e6ace54e3f6bafe4077ade57924422b5eb3baf4dbd23c-20260522-222729.md`*

Compacted content:
- Archived 1 response topic(s): graph performance next steps
- Prior summary/contet: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc6544...
### Re: graph performance queue — gpt-5

Completed the graph performance queue items that were locally actionable.

Implemented in `src/tsift`:
- Graph refresh now streams materialized `graph_node_properties` rows while node rows are staged, avoiding the extra `json_each(next_graph_nodes.properties_json)` pass over the staged projection.
- Context-pack exploration rows now use one batched SQLite projection transaction for `source_handle`, `worker_context`, and relationship rows instead of per-row autocommit writes.
- Added regression coverage for the batched SQLite projection upsert path and updated README/SPEC/VERSIONS plus `fixtures/graph-db-performance-history.json` with the fresh real agent-loop baseline.

Commits:
- `src/tsift` `c0ebfd5` (`perf: batch graph projection property writes`)
- superproject `851d29aad` (`tsift: batch graph projection writes`)

Performance proof from real root `backend-eval` after installing the rebuilt CLI:
- Immediate before -> after: SQLite refresh `28.02s -> 24.41s`, source graph build `16.59s -> 14.20s`, projection rows `2.86s -> 2.24s`, SQLite delta write `8.57s -> 7.97s`, conflict prep `6.09s -> 4.80s`, dispatch trace `1.58s -> 1.49s`, path_max_hops `1.10s -> 1.03s`, SQLite total `33.00s -> 29.20s`.
- Candidate totals after: DuckDB/DuckPGQ `6.53s`, FalkorDB `6.17s`, Ladybug `6.41s`; all remain `hold` because real parity differed from SQLite, and Ladybug also missed the SQLite `path_max_hops` time.
- Metric digest against the prior fixture shows candidate totals improved about 87%, while SQLite still records a larger-source-graph regression versus the older baseline; the new run is now recorded as `real-agent-loop-batched-projection-2026-05-22`.

Compaction / Convex proof:
- Local Convex dry-run planned `351703` node upserts, `586611` edge upserts, and `9385` chunks; no network mutation was attempted.
- Real `convex-sync --remote-snapshot --apply` is externally blocked because neither `--endpoint` nor `TSIFT_CONVEX_GRAPH_URL` is configured in this shell.
- `graph-db compact --apply` reclaimed storage locally: file size `1012051968 -> 975118336` bytes and freelist `618496 -> 0`; tombstones remain retained pending Convex reconciliation.

Verification:
- Focused tests for projection refresh, batched projection upsert, and context-pack graph materialization passed.
- `make check` passed after the final fixture update: `653` unit tests passed (`1` ignored), `108` exit-code tests passed, and `30` graph-db conformance tests passed (`1` live Convex test ignored).
- `cargo install --path . --locked` replaced the local `tsift` binary; `tsift --version` reports `0.1.42`.
- Latest `btakita/tsift` GitHub `CI` before this new push was green (`26314205965`). Latest root `Agent Doc` run `26315225081` failed with empty `check` and `tmux-ci` job step lists and `gh run view --log-failed` returned `log not found`, so I classified it as the same external CI-start blocker rather than a code or tmux regression.

Per agent-doc closeout ordering, the local commits are ready and the pushes follow after this document response is finalized.
### Re: gdbrowstream queue cleanup — gpt-5

`#gdbrowstream` is already resolved by the prior closeout in `src/tsift` commit `c0ebfd5` and superproject commit `851d29aad`; no additional repo work was needed for this queue step. The active queue line was stale after the first auto-queue cycle consumed only `#gdbpostbase`.

Verification and push status remain unchanged: final `make check` passed and `btakita/tsift` CI run `26315623600` passed. Root `Agent Doc` run `26315620142` still failed before job steps/logs were available, so it remains an external CI-start blocker.
### Re: gdbprepstage queue cleanup — gpt-5

`#gdbprepstage` is also already resolved by the prior closeout in `src/tsift` commit `c0ebfd5` and superproject commit `851d29aad`. The planner found this as queue-head drift with an existing completed worker result, so I did not redispatch or change repository files.

The verification record remains the same: final `make check` passed, `btakita/tsift` CI run `26315623600` passed, and the root `Agent Doc` run `26315620142` remains classified as an external CI-start blocker because its jobs had no steps or logs.
### Re: gdbvacproof queue cleanup — gpt-5

`#gdbvacproof` remains review-gated rather than done. Local evidence exists for the parts that could run without live Convex credentials: Convex dry-run produced a reconciliation plan, `graph-db compact --apply` reduced the SQLite graph file from `1012051968` to `975118336` bytes with freelist `618496 -> 0`, and post-compact status showed freelist `0`.

The unresolved blocker is external: live Convex reconciliation failed because `TSIFT_CONVEX_GRAPH_URL` / `--endpoint` was not configured. I left the review-gated item in place and did not change repo files for this queue cleanup.
### Re: gdbenginegate queue cleanup — gpt-5

`#gdbenginegate` was already completed in the prior implementation closeout: the backend-eval run kept DuckDB/DuckPGQ, FalkorDB, and Ladybug behind promotion gates because they still diverged from SQLite parity, and the result was committed in `src/tsift` commit `c0ebfd5` plus superproject commit `851d29aad`.

No repository files changed for this queue cleanup. The final verification state remains: `make check` passed locally, `btakita/tsift` CI run `26315623600` passed, and root `Agent Doc` run `26315620142` remains an external CI-start blocker due empty jobs with no logs.
❯ Are the canditate DuckDB + FalkorDB now faster than SqliteDB?
### Re: candidate backend speed — gpt-5

Yes on raw elapsed time in the latest backend-eval, but no for promotion.

The latest real run measured SQLite at `29.199s` total after the batching change. The candidate totals were faster: DuckDB/DuckPGQ at `6.529s` and FalkorDB at `6.170s`. So as benchmark candidates, both are currently faster than SQLite on that measured total.

They still cannot replace SQLite because both failed the real parity gate: DuckDB/DuckPGQ differed from SQLite, and FalkorDB also differed from SQLite. Until the parity mismatches are fixed, the correct status is "faster candidate, not promotable backend." SQLite remains the authoritative backend.
❯ Did you test LadybugDB? Is Vela-Engineering/kuzu a good candidate?
### Re: LadybugDB and Kuzu candidates — gpt-5

Yes, LadybugDB was tested in the latest `graph-db backend-eval`. It was fast on raw total time: `6.409s` versus SQLite at `29.199s`. It still failed the promotion gate because real parity differed from SQLite, and its `path_max_hops` probe was slightly slower than SQLite: `1.039s` versus `1.029s`. So LadybugDB is in the same bucket as DuckDB/DuckPGQ and FalkorDB: fast candidate, not promotable yet.

`Vela-Engineering/kuzu` looks like a credible candidate to evaluate, but it has not been tested in this tsift backend-eval run. The case for testing it is strong: Kuzu is an embedded property graph database with Cypher, columnar/CSR storage, vectorized execution, Rust bindings, and no server requirement; the current Rust crate docs also show an in-process `kuzu` API. Sources: [Kuzu docs](https://docs.kuzudb.com/get-started/), [kuzu crate](https://docs.rs/kuzu/latest/kuzu/). The Vela fork specifically claims active maintenance, MIT licensing, and concurrent multi-writer support for multi-agent systems, with a latest GitHub release shown as May 19, 2026: [Vela-Engineering/kuzu](https://github.com/Vela-Engineering/kuzu).

My read: Kuzu/Vela is worth benchmarking as a real engine adapter, not just a row-adapter prototype. It is especially interesting if concurrent graph writes become important. The main risks to prove before treating it as better than SQLite are parity against the `GraphStore` contract, install/build portability in Rust, schema mapping for tsift's provider-neutral node/edge/property rows, and multi-process lock behavior under simultaneous agent workflows.
❯ Looking at how tsift is being used, what data store do you recommend investing development into?
### Re: datastore investment recommendation — gpt-5

I would invest first in SQLite, then run a focused Vela-Kuzu spike. I would not put more primary effort into DuckDB/DuckPGQ, FalkorDB, or Ladybug until their parity failures are understood.

SQLite should stay the production correctness store because that is how tsift is actually used today: local CLI workflows, offline agent-doc sessions, deterministic fixtures, lock recovery, snapshot fallback, and small portable `.tsift` databases. The latest measured SQLite total is slower than candidates (`29.199s` versus roughly `6.17s` to `6.53s`), but SQLite is the only backend currently carrying the full correctness, install, and operator contract. SQLite WAL also fits the current read-heavy shape: readers and a writer can coexist, with the known limitation that writes still serialize. Source: [SQLite WAL docs](https://www.sqlite.org/wal.html).

The best next investment candidate is Vela-Engineering/kuzu, not FalkorDB or Ladybug. Kuzu's model matches tsift better than the current row-adapter prototypes: embedded graph database, Cypher/property graph semantics, Rust API, and no required server. The Vela fork is specifically positioned for multi-agent concurrent writers, which is the one serious weakness SQLite will hit if several agents start refreshing or materializing graph rows at once. Sources: [Kuzu docs](https://docs.kuzudb.com/), [Kuzu Rust tutorial](https://kuzudb.com/docs/tutorials/rust/), [Vela Kuzu fork rationale](https://www.vela.partners/blog/kuzudb-ai-agent-memory-graph-database).

FalkorDB is still interesting, but it adds Redis/server operational weight, which cuts against tsift's local-first CLI usage; its docs describe it as a Redis module. Source: [FalkorDB docs](https://docs.falkordb.com/). Ladybug was already measured and is fast, but it failed parity and does not address the multi-writer issue as directly as the Vela fork claims to.

So my recommendation is a two-track investment: keep optimizing SQLite's projection/write path as the reliable default, and add one serious Vela-Kuzu backend-eval adapter to answer whether an embedded graph engine can beat SQLite while preserving parity and local install ergonomics. If Kuzu/Vela fails parity or build portability, fall back to SQLite without regret.
<!-- agent:boundary:917f3ad0 -->
<!-- /agent:exchange -->
###
<!--
-->

<!-- agent:queue -->
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog -->
- [ ] [#gdbkuzu] [recommended] Prototype a Vela-Engineering/kuzu GraphStore adapter behind backend-eval, including parity, Rust build/install portability, projection load, concurrent writer behavior, and real/synthetic performance gates before considering any SQLite replacement.
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
