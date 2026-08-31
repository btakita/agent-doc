---
agent_doc_session: tsift-v0.1
resume:
  claude: 7ec8a9b0-c6b3-46ca-b057-dfbb94db6d6d
  codex: 01a044ce-1438-7c20-b3d6-07469fefa0f0
agent: codex
claude_model: opus
agent_doc_format: template
agent_doc_write: crdt
claude_args: --dangerously-skip-permissions
codex_args: -s danger-full-access
prompt_presets:
  '#advance-review': Go through the review items. Add actionable backlog items to complete or advance the review items.,
  '#agent-doc-bug': Please create a plan for agent-doc to fix this issue. Add to the backlog of agent-loop/tasks/agent-doc/agent-doc-bugs2.md
  '#bugs-observed': Please add any bugs from observed behavior to the backlog.,
  '#code-review': Please review tsift codebase. Inspect the architecture and look for edge cases. Isolate concurrency issues and other workflow bugs. Let's find root causes to fix. '#follow-up-backlog'
  '#mermaid': Include explanatory mermaid diagram(s).
  '#next-steps': Any follow-up items to place in the backlog? Please ensure the backlog is in priority order. Print the item id of the top of the backlog.
  '#spec-test-build-install-commit-push': update spec + tests. build + install for local testing. commit + push
  '#spec-test-commit-push': update spec + tests. commit + push
queue: stop
---

## Status

<!-- agent:status patch=replace -->

complete
<!-- /agent:status -->

## Spec

See [src/tsift/SPEC.md](../../src/tsift/SPEC.md) for the AST-Aware Code RAG spec.

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc65443c45f7b49f5e6ace54e3f6bafe4077ade57924422b5eb3baf4dbd23c-20260831-175607.md`*

Compacted content:
- Archived 2 response topic(s): issue #21 and tsift 0.1.93; do [#fixopentsift]
- Prior summary/context: prior compacted content: Archived 25 response topic(s): Fix tsift issues 1–3; release + publish + install; Fix tsift ...

### Re: tsift issue #23 and v0.1.95 — gpt-5

> **Queue prompt:** Please fix the open issue in tsift. Release + publish 0.1.95.

Completed issue #23 and released tsift 0.1.95.

- Graph freshness now hashes the semantic index rows that actually feed projection materialization, and the projection contract includes the tsift package version. `graph-db status`, doctor, and context-pack fail closed when index rows or the projection version no longer match.
- Added the supported `graph-db refresh --rebuild` recovery path, including operator diagnostics and conformance coverage.
- Federated inline index repair now reuses descendant-scope exclusions, so parent scopes do not re-admit nested-owned files and repeated refreshes remain idempotent.
- Added regressions for route-row removal, projection-version mismatch, forced rebuild, and nested-scope refresh.

Release proof:

- Fix commit: `c9b75a2` (`Fixes #23`)
- Release commit/tag/main: `329d7a2` / `v0.1.95`
- GitHub release: https://github.com/btakita/tsift/releases/tag/v0.1.95 with all 6 expected archives/checksums
- crates.io: all 26 workspace packages independently resolved at `0.1.95`
- npm: `opencode-tsift@0.1.95` is `latest`
- Issue #23 is closed
- Local verification: focused regression suites plus two clean `make check` runs
- Hosted CI: https://github.com/btakita/tsift/actions/runs/33426805057 — success
- Release workflow: https://github.com/btakita/tsift/actions/runs/33426837542 — success

The original dirty `src/tsift` checkout was left untouched; the release was built from an isolated clean worktree.
<!-- agent:boundary:b8ec92e9:tsift -->
<!-- /agent:exchange -->

## Queue

<!-- agent:queue preset="#spec-test-build-install-commit-push" priority start -->
<!-- /agent:queue -->
<!--
https://github.com/chopratejas/headroom
-->

## Backlog

<!-- agent:backlog priority queue -->
<!-- /agent:backlog -->

## Review

<!-- agent:review -->
<!-- /agent:review -->

<!-- agent:icebox -->
- [ ] [#g5kw] What are #next-steps to promote SurrealDB to SQLite-equivalent substrate status?
- [ ] [#4z0w] Can we increase max-hop caps?
- [ ] [#cg52] Could FalkorDB help with performance?
<!-- /agent:icebox -->

## Completed / Reaped

<!-- agent:done archive=tasks/software/tsift.done.md -->

<!-- completed work archived in tasks/software/tsift.done.md -->
<!-- /agent:done -->

<!-- agent:notes -->
• Ran tsift rewrite --run 'git log -5 --oneline --decorate'
└ tsift rewrite: no rewrite: no supported tsift rewrite matched this command; `--run` executes only rewritten commands; run the original command directly if intended

#spec-test-build-install-commit-push
---
route duplicate prompt residue guard failed: failed to parse components: unclosed component: `agent:notes` without matching close
<!-- /agent:notes -->
