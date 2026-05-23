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

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc65443c45f7b49f5e6ace54e3f6bafe4077ade57924422b5eb3baf4dbd23c-20260523-001050.md`*

Compacted content:
- Archived 8 response topic(s): graph performance queue; gdbrowstream queue cleanup; gdbprepstage queue cleanup; 5 more
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc6544...
❯ do [#gdbkuzu]. #spec-test-build-install-commit-push
### Re: kuzu backend-eval prototype — gpt-5

Completed `#gdbkuzu`.

Implemented a dependency-free `kuzu` backend-eval candidate in `src/tsift`, alongside DuckDB/DuckPGQ, FalkorDB, and Ladybug. The candidate uses the existing provider-neutral `GraphStore` row projection path, reports Kuzu-specific projection-load behavior, concurrent-writer/lock behavior, and Rust build/install portability notes, and remains gated behind `graph-db backend-eval` rather than changing SQLite as the correctness substrate.

Changed paths:
- `src/tsift/src/main.rs`
- `src/tsift/tests/graph_db_conformance.rs`
- `src/tsift/SPEC.md`
- `src/tsift/README.md`

Verification:
- `cargo test --test graph_db_conformance graph_db_backend_eval_benchmarks_candidate_stores_against_sqlite -- --nocapture`
- `make check`
- `cargo build --locked`
- `cargo install --path . --locked`
- `tsift --version`
- `tsift graph-db --path . backend-eval --help`
- `tsift graph-db --path . --json schema`

CI review:
- `gh run list --workflow CI --limit 1` in `src/tsift`: latest `tsift` CI run `26315623600` was successful.
- Root `Agent Doc` latest run `26318080821` has empty-step failures for `tmux-ci` and `check`; `gh run view --job 77481397502 --log` returned `log not found`, so I treated it as an external CI-start blocker rather than a tsift/tmux regression.

Commits:
- `src/tsift`: `82dac57 feat: add kuzu backend eval candidate`
- superproject: `36171dc13 chore: pin tsift kuzu backend eval`

Push is queued immediately after this mandatory agent-doc closeout so the session document stays on the finalize path.
<!-- agent:boundary:3f487d74 -->
<!-- /agent:exchange -->
###
<!--
What are #next-steps to improve the sqlitedb graph performance?
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
