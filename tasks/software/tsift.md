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

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc65443c45f7b49f5e6ace54e3f6bafe4077ade57924422b5eb3baf4dbd23c-20260520-202137.md`*

Compacted content:
- Archived 2 response topic(s): graph DB API next steps; graph DB API implementation
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc6544...
❯ What are #next-steps in support graph db api over sqlite and convex?
### Re: graph DB API next steps — gpt-5

Top backlog item after this cycle: `#cvxa`.

The core support is already in place: `GraphStore` is the stable substrate, SQLite is the local correctness store, `tsift graph-db` reads SQLite and Convex snapshots, `convex-sync --apply` has a transport path, and conformance/freshness coverage exists. The next work should move from internal API support to operational Convex consumption and scale hardening.

I added three prioritized follow-ups:

1. `#cvxa` — ship the reusable Convex app-side backend package: schema, indexes, mutations/actions that accept the `convex-sync` payload, plus a smoke test proving `--apply` round-trips against the documented endpoint.
2. `#gqpg` — harden graph-db reads for large graphs with pagination, property filtering, deterministic backend ordering, cap diagnostics, and parity tests over larger SQLite/Convex snapshot fixtures.
3. `#gdoc` — add operator-facing examples and fixtures that show SQLite `graph-db` queries, Convex dry-run/apply payloads, stale snapshot failures, and `context-pack`/`traverse` handle reuse.

I kept `#98qd` immediately after those because agent-doc context shaping should consume the stable graph DB API once the Convex backend contract and large-query behavior are explicit.
<!-- agent:boundary:16dbabad -->
<!-- /agent:exchange -->
###
<!--
-->

<!-- agent:queue -->
dispatch #spec-test-build-install-commit-push
- do [#gapi]
- do [#gcon]
- do [#cxlv]
- do [#gidx]
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog -->
- [ ] [#cvxa] Add a reusable Convex graph backend package: schema, indexes, mutations/actions matching tsift convex-sync payloads, plus a local/hosted smoke test proving --apply round-trips against the documented endpoint.
- [ ] [#gqpg] Add graph-db query pagination and filtering across SQLite and Convex snapshots: cursor/limit semantics, property filters, deterministic ordering, cap diagnostics, and conformance coverage over large projection fixtures.
- [ ] [#98qd] Support agent-doc context shaping/orchestration: add tsift-backed backlog coverage for job packets, source handles, and bounded worker context.
- [ ] [#gdoc] Add graph-db operator examples and fixtures showing SQLite graph-db queries, convex-sync dry-run/apply payloads, stale Convex snapshot failures, and context-pack/traverse handle reuse.
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
