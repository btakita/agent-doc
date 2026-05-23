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

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc65443c45f7b49f5e6ace54e3f6bafe4077ade57924422b5eb3baf4dbd23c-20260523-213135.md`*

Compacted content:
- Archived 1 response topic(s): graph performance next steps
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc6544...
### Re: graph backend eval queue — gpt-5

Implemented `#gdbprepprof`, `#gdbprofilefull`, `#gdbhoptiers`, and `#gdbfalkorreal` in `src/tsift`.

Changed paths:
- `src/tsift/src/main.rs`
- `src/tsift/tests/graph_db_conformance.rs`
- `src/tsift/SPEC.md`
- `src/tsift/README.md`
- `src/tsift/VERSIONS.md`
- `src/tsift/fixtures/graph-db-performance-history.json`

What changed:
- Conflict-matrix preparation now exposes split timings for cache lookup, session-review compute, status/index gate, context-pack diff, exploration materialization, graph orchestration, staged diff, and impact; it also has an in-process source/document/staged-diff cache key.
- `graph-db backend-eval` now accepts `--full-projection`, adds an opt-in full-project dataset, and reports that profile in the config.
- Path probes now include the 64-hop default plus measured 128/256-hop deep-chain tiers, with query-plan checks and fixture metrics.
- FalkorDB remains explicitly held behind backend-eval until a production adapter beats SQLite on full-projection conflict-matrix, evidence, dispatch-trace, path-tier, install-portability, and lock-behavior gates.

Verification:
- `cargo fmt`
- `cargo check`
- `cargo test --test graph_db_conformance` passed: 31 tests.
- `make check` passed: clippy plus full test suite, including 658 main tests, 108 `exit_code` tests, and 31 graph DB conformance tests.
- `cargo install --path . --force` passed; installed `tsift 0.1.42`.
- Backend-eval conformance benchmark coverage passed through `graph_db_conformance` and the updated performance fixture.

Live full-queue benchmark note: I attempted the real agent-loop backend-eval queue run with all four targets, with and without `--full-projection`, plus bounded Falkor-only debug and release probes. Those runs stayed pre-output for 120s-385s and produced zero-byte JSON, so I am recording that as the current live queue performance blocker rather than claiming a successful live benchmark sample.

CI/tmux evidence: after pushing, the latest `Agent Doc` workflow run `26344780487` for root commit `691529147` failed before any runner steps. Both `check` and `tmux-ci` have empty step lists and GitHub's billing/spending-limit annotation, so this is an external CI-start blocker; under the runbook, no local `make tmux-ci` rerun was required from that CI state.

Pushed commits:
- `src/tsift`: `eedb829 feat: profile graph backend eval gates`
- `agent-loop`: `691529147 chore: update tsift backend eval gates`

Confidence: high for the implemented API/reporting/test surfaces covered by the conformance suite; medium for live full-project runtime behavior until backend-eval emits JSON on the real queue target set.
<!-- agent:boundary:f997c529 -->
<!-- /agent:exchange -->
###
<!--

#spec-test-build-install-commit-push
-->

<!-- agent:queue -->
preset #spec-test-build-install-commit-push
- do [#gdbprepprof]
- do [#gdbprofilefull]
- do [#gdbhoptiers]
- do [#gdbfalkorreal]
- Please run a benchmark on the performance improvements for this queue run
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
