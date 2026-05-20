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

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc65443c45f7b49f5e6ace54e3f6bafe4077ade57924422b5eb3baf4dbd23c-20260520-223256.md`*

Compacted content:
- Archived 1 response topic(s): graph-db projection coverage
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc6544...
❯ What are the #next-steps for graphdb functionality?
### Re: graphdb next steps — gpt-5

The next graphdb work should move from substrate coverage to operational usability.

Top backlog item: `#gref`.

Three ordered follow-ups are now in the backlog:

1. `#gref`: explicit `graph-db` refresh/status workflow so operators can materialize and inspect `.tsift/graph.db` directly, then get concrete `doctor`, `drift`, and `convex-sync` next commands.
2. `#guse`: agent-facing graph-db evidence packets for queued/backlog work, built around `worker_context`, `source_handle`, shortest paths, and recommended commands.
3. `#gliv`: promote the live Convex graph backend acceptance path into a documented release or CI gate once a dedicated deployment and secrets are available.

I would keep the graphify semantic and visualization items in icebox until `#gref` and `#guse` are implemented. The graph DB first needs a reliable operator path and a first-class agent handoff surface.
<!-- agent:boundary:c32391b1 -->
<!-- /agent:exchange -->
###
<!--
-->

<!-- agent:queue -->
preset #spec-test-build-install-commit-push
- do [#grwp]
- do [#gdrf]
- do [#gcap]
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog -->
- [ ] [#gref] Add an explicit graph-db refresh/status operator workflow that materializes .tsift/graph.db without requiring a throwaway traverse call, reports projection version/content hash/source watermark/tombstone counts, and emits next commands for doctor, drift, and convex-sync.
- [ ] [#guse] Add a graph-db evidence packet for queued or backlog work: start from a backlog id or job packet handle, return bounded worker_context and source_handle rows, shortest paths, recommended next commands, and compiled fixture coverage for agent handoffs.
- [ ] [#gliv] Promote the live Convex graph backend acceptance path into a documented release or CI gate once a dedicated deployment and secrets exist, including graph-db node/kind/neighborhood/path parity evidence from the remote snapshot.
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
