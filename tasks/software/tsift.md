---
agent_doc_session: tsift-v0.1
resume: 019dd526-7958-77e3-9a41-92b8944fc550
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
Project created. Repo: https://github.com/btakita/tsift. Submodule: src/tsift. Spec extracted to src/tsift/SPEC.md.
<!-- /agent:status -->

## Spec

See [src/tsift/SPEC.md](../../src/tsift/SPEC.md) for the AST-Aware Code RAG spec.

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc65443c45f7b49f5e6ace54e3f6bafe4077ade57924422b5eb3baf4dbd23c-20260520-214623.md`*

Compacted content:
- Archived 1 response topic(s): graph DB conformance
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc6544...
❯ What are the #next-steps for graph db behavior?
### Re: graph DB behavior next steps — gpt-5

Top backlog item after this cycle: `#grwp`.

The previous graph DB behavior batch is now complete: `#gval`, `#gobs`, and `#gfed` are all archived as done, and the spec now has SQLite/Convex conformance, doctor diagnostics, and opt-in live Convex acceptance coverage. The next step should be proving the behavior against realistic workspace graphs and operational drift, rather than adding a wider API surface.

I added these follow-ups in priority order:

1. `#grwp` — prove graph-db behavior on a real agent-loop/tsift workspace fixture, including stable `session`, `backlog`, `source_handle`, and `worker_context` rows.
2. `#gdrf` — add a graph projection drift/diff report between SQLite and Convex snapshots so operators can see concrete upserts, tombstones, stale metadata, and fail-closed repair commands before apply/read paths.
3. `#gcap` — add graph-db scale/cap regression coverage for deterministic pagination, property filters, bounded neighborhood/path limits, doctor runtime, and SQLite query-plan/index expectations.

I placed them ahead of crates.io/release cleanup because they are still graph DB behavior hardening. `#atvp` and `#g6kf` remain below them as packaging/release follow-up work.
<!-- agent:boundary:14c9649a -->
<!-- /agent:exchange -->
###
<!--
-->

<!-- agent:queue -->
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog -->
- [ ] [#grwp] Prove graph-db behavior on a real agent-loop/tsift workspace fixture: refresh .tsift/graph.db, capture stable node/kind/neighborhood/path examples for session, backlog, source_handle, and worker_context rows, and lock them in compiled CLI or fixture coverage so the substrate is validated beyond toy projects.
- [ ] [#gdrf] Add a graph projection drift/diff report for SQLite vs Convex snapshots that summarizes node/edge upserts, tombstones, stale projection metadata, orphan/duplicate failures, and operator next commands before --apply or graph-db reads.
- [ ] [#gcap] Add graph-db scale/cap regression coverage over a large synthetic traversal projection: deterministic pagination through all pages, property filter selectivity, bounded neighborhood/path limits, doctor runtime, and SQLite index/query-plan assertions.
- [ ] [#atvp] Unblock crates.io publication for tsift by replacing the upstream git-only sift dependency, publishing a compatible upstream crate, or vendoring a small stable search adapter; rerun cargo publish --dry-run before enabling TSIFT_ENABLE_CRATES_PUBLISH.
- [ ] [#g6kf] Confirm the v0.1.42 GitHub release completes after the macOS x86_64 Actions runner leaves the queue; if it remains stuck, remove or replace that matrix target and retag a fixed release.
<!-- /agent:backlog -->

<!-- agent:icebox -->
- [ ] [#9jc8] graphify integration: LLM-based entity extraction for semantic layer beyond AST
- [ ] [#zv4m] graphify integration: HTML visualization of symbol/dependency graphs
- [ ] [#rd2f] graphify integration: semantic similarity between concepts (embedding-based)
<!-- /agent:icebox -->

## Completed / Reaped

<!-- agent:done archive=tasks/software/tsift.done.md -->

<!-- completed work archived in tasks/software/tsift.done.md -->
<!-- /agent:done -->
