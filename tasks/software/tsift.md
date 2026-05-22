---
agent_doc_session: tsift-v0.1
agent_doc_format: template
agent_doc_write: crdt
agent: codex
claude_model: opus
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
Completed: #gdbrefreshhot, #gdbmetricreal, #gdbtracehot, #gdbhopcaps, #gdbcompactrun, and #gdbfalkorgate shipped in tsift 638fe4c and superproject 9a5447b88; graph backend evaluation now records real metrics, phase timings, path cap probes, and gated FalkorDB evidence.
<!-- /agent:status -->

## Spec

See [src/tsift/SPEC.md](../../src/tsift/SPEC.md) for the AST-Aware Code RAG spec.

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc65443c45f7b49f5e6ace54e3f6bafe4077ade57924422b5eb3baf4dbd23c-20260522-040703.md`*

Compacted content:
- Archived 1 response topic(s): graph performance next steps
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc6544...
### Re: graph backend eval gates — gpt-5

Completed `#gdbrefreshhot`, `#gdbmetricreal`, `#gdbtracehot`, `#gdbhopcaps`, `#gdbcompactrun`, and `#gdbfalkorgate`.

Shipped and pushed:
- `src/tsift` commit `638fe4c` (`feat: expand graph backend eval gates`)
- superproject commit `9a5447b88` (`chore: update tsift graph backend eval`)

What changed:
- Backend evaluation now includes FalkorDB as an experimental gated candidate with promotion decision evidence; the current decision is `hold` because parity/performance did not beat SQLite.
- Refresh timing is split into phase metrics for source graph build, projection rows, SQLite open, SQLite delta write, and conflict-matrix preparation.
- Backend metric digests now include real phase metrics and compare generated current reports against the checked-in performance fixture.
- Dispatch-trace timing reuses the conflict-matrix report instead of recomputing the expensive read path.
- Path probing now includes `path_max_hops` with bounded/adaptive caps: 64 for synthetic deep-chain coverage and 1 for high-degree/direct-edge probes to avoid recursive CTE blowups.
- `fixtures/graph-db-performance-history.json`, `SPEC.md`, `README.md`, and graph-db conformance tests were updated with real 2026-05-22 agent-loop evidence.

Real graph evidence from `tasks/software/tsift.md`:
- 351,069 nodes and 579,982 edges in the backend-eval run.
- SQLite total `24,721,237us`; refresh `19,598,243us`; status `1,319,068us`; `path_max_hops` `1,023,440us`; evidence `28,128us`; conflict-matrix `1,211,143us`; dispatch-trace `1,541,215us`.
- Refresh phase timings: source graph build `4,358,978us`; projection rows `2,286,601us`; SQLite open `10,691us`; SQLite delta write `12,941,973us`; conflict-matrix preparation `5,158,398us`.
- FalkorDB placeholder/gate evidence recorded real total `49,458,654us` and conflict-matrix `40,426,179us`, so it stays behind the promotion gate.

Operational compact evidence:
- Convex sync was run as a local dry-run only: 351,028 node upserts, 579,917 edge upserts, 9,311 chunks. No remote Convex apply occurred because no remote snapshot/endpoint was configured.
- Refresh before compact produced 351,028 nodes, 579,917 edges, and 188,818 tombstones.
- Compact apply reduced `graph.db` from `1,136,963,584` bytes to `951,001,088` bytes and freelist from about `133,042,176` bytes to `0`.
- Tombstones remain gated: `safe_to_prune_tombstones=false`, `requires_convex_reconciliation=true`.
- Post-compact doctor returned `status=ok` with no repair commands.

Verification:
- `cargo test graph_db_backend_eval_benchmarks_candidate_stores_against_sqlite --test graph_db_conformance`
- `make check` in `src/tsift` passed: clippy, full tests, and graph-db conformance with 30 passed / 1 ignored.
- `cargo build --release` passed.
- `cargo install --path . --force` passed; installed `tsift --version` reports `tsift 0.1.42`.
- tsift GitHub Actions run `26268712948` passed for commit `638fe4c`; both `clippy + test` and `live Convex graph acceptance` jobs completed successfully. The live Convex job emitted only the documented notice that `TSIFT_LIVE_CONVEX_GRAPH_URL` is needed for the remote parity gate.
- superproject GitHub Actions run `26268726869` did not start `tmux-ci` or `check` because GitHub reported account billing/spending-limit exhaustion. I classified that as an external CI-start blocker, not a code or tmux regression.
<!-- agent:boundary:f84774d6 -->
<!-- /agent:exchange -->
###
<!--
-->

<!-- agent:queue -->
preset #spec-test-build-install-commit-push
- do [#gdbrefreshhot]
- do [#gdbmetricreal]
- do [#gdbtracehot]
- do [#gdbhopcaps]
- do [#gdbcompactrun]
- do [#gdbfalkorgate]
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog -->
- [/] [#orchdog] [recommended] Dogfood the completed graph-backed agent orchestration stack on a real agent-doc queue run: run refresh -> evidence -> conflict-matrix -> dispatch-trace -> jobs create/collect, compare worker_prompt_packet and worker_result closure fields, and capture any tsift-side fixture gaps. **Dogfood performed 2026-05-21 on `session-share/tasks/root.md` items #x63e + #v4v0; findings recorded in #gdbevid, #cmgr, #gdbidx — see `session-share/tasks/root.md` exchange "graph-backed orchestration dogfood" entry for the full pipeline trace.** **Second dogfood 2026-05-21 against `tasks/software/tsift.md` (preset `#dogfood-tsift`) surfaced 2 new gaps: #cmgrwres and #gdblock.**
<!-- /agent:backlog -->
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
