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

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc65443c45f7b49f5e6ace54e3f6bafe4077ade57924422b5eb3baf4dbd23c-20260524-123945.md`*

Compacted content:
- Archived 4 response topic(s): graph backend cache queue; benchmark queue closeout; Please run a benchmark on the performance improvements for this queue run; 1 more
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc6544...
❯ do [#gdbcachewm]. #spec-test-build-install-commit-push
### Re: gdbcachewm — gpt-5

Completed `#gdbcachewm`.

Changed paths:
- `src/tsift/SPEC.md`
- `src/tsift/src/main.rs`
- `src/tsift/tests/graph_db_conformance.rs`
- superproject gitlink `src/tsift`

What changed:
- Fixed generated-artifact detection so relative index snapshot paths such as `.tsift/conflict-matrix-cache/...` are treated as generated artifacts.
- Filtered generated `.tsift` and `target` paths out of traversal graph inputs loaded from existing index rows: file nodes, symbols, call edges, and route rows.
- Updated the spec to state that conflict-matrix cache artifacts are excluded from source watermarks and indexed graph inputs.
- Added a backend-eval regression that seeds a stale `.tsift/conflict-matrix-cache` index row, proves it is not projected as file/symbol graph input, and proves the second and third backend-eval CLI runs both reuse the conflict-matrix disk cache.

Verification:
- `cargo test --test graph_db_conformance graph_db_backend_eval_benchmarks_candidate_stores_against_sqlite -- --nocapture` passed.
- `make check` passed: clippy, 658 unit tests, 108 exit-code integration tests, and 31 graph-db conformance tests.
- `cargo build` passed.
- `cargo install --path . --force` installed `tsift v0.1.42` locally.
- `git diff --check` passed.
- tsift GitHub `CI` run `26361745103` completed successfully after push.
- superproject `Agent Doc` run `26361755591` failed before runner steps were allocated; both `tmux-ci` and `check` jobs had empty step lists, so I classified it as the known external CI-start blocker rather than a code or tmux regression.

Commits and push:
- `btakita/tsift` main: `622a3ff` (`fix: keep conflict matrix cache out of graph inputs`) pushed.
- `btakita/agent-loop` main: `d1f942331` (`chore: update tsift submodule`) pushed.

Confidence: high. Escalation: none.
<!-- agent:boundary:56740ded -->
<!-- /agent:exchange -->
###
<!--
#spec-test-build-install-commit-push
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
