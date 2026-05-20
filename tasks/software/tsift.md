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

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc65443c45f7b49f5e6ace54e3f6bafe4077ade57924422b5eb3baf4dbd23c-20260520-205717.md`*

Compacted content:
- Archived 2 response topic(s): graph DB API next steps; graph DB hardening implementation
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc6544...
❯ What are #next-steps for graph db behavior?
### Re: graph DB behavior next steps — gpt-5

Top backlog item after this cycle: `#gval`.

The previous queued graph DB batch already moved the substrate from API support into working SQLite/Convex behavior: reusable Convex app package, paged/filterable reads, agent-doc/context-pack graph rows, and operator examples are complete. The next work should protect that behavior as a contract rather than widening the surface again.

I added these follow-ups in priority order:

1. `#gval` — add a compiled CLI conformance suite shared across SQLite and Convex snapshots, covering fail-closed schema/freshness behavior, transactional refresh visibility, tombstone order, missing-node edge rejection, pagination/filter parity, and deterministic ordering.
2. `#gobs` — add `graph-db` validation/doctor diagnostics so operators can detect stale metadata, missing Convex indexes, orphan edges, duplicate ids, schema drift, and get concise repair commands with fail-closed exit codes.
3. `#gfed` — add an opt-in live Convex acceptance harness that proves the example backend can apply a projection, pull the remote snapshot, and pass node/kind/neighborhood/path parity end to end.

I placed those ahead of crates.io/release cleanup because they are behavioral contract work for the graph DB API. Publishing should come after the graph DB behavior is harder to regress.
<!-- agent:boundary:a4c96ecd -->
<!-- /agent:exchange -->
###
<!--
-->

<!-- agent:queue -->
dispatch #spec-test-build-install-commit-push
- do [#cvxa]
- do [#gqpg]
- do [#98qd]
- do [#gdoc]
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog -->
- [ ] [#gval] Lock graph DB behavior with a compiled CLI conformance suite shared across SQLite and Convex snapshots: schema-version fail-closed checks, transactional refresh visibility, tombstone reconciliation order, missing-node edge rejection, freshness failures, pagination/filter parity, and deterministic ordering.
- [ ] [#gobs] Add graph-db validation/doctor diagnostics for SQLite graph.db and Convex snapshots: stale projection metadata, missing required Convex indexes, orphan edges, duplicate ids, schema drift, and concise repair commands with fail-closed exit codes.
- [ ] [#gfed] Add an optional live Convex graph backend acceptance harness: deploy or target the example backend, apply a small projection, pull the remote snapshot, and run graph-db node/kind/neighborhood/path parity end-to-end with credentials documented as opt-in.
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
