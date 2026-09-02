---
agent_doc_session: tsift-v0.1
resume:
  claude: 7ec8a9b0-c6b3-46ca-b057-dfbb94db6d6d
  codex: 01a0599a-9461-7981-b7a8-068420950fbf
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

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc65443c45f7b49f5e6ace54e3f6bafe4077ade57924422b5eb3baf4dbd23c-20260831-205459.md`*

Compacted content:
- Archived 1 response topic(s): tsift issue #23 and v0.1.95
- Prior summary/context: prior compacted content: Archived 2 response topic(s): issue #21 and tsift 0.1.93; do [#fixopentsift]

### Re: tsift 0.1.96 released — gpt-5.6

> **Queue prompt:**
>
> do [#fixopenissues]


Completed `#fixopenissues`.

- Fixed #24 by canonicalizing graph watermark path hints, persisting the projection path and mode, using path-aware freshness for every SQLite graph read, and making stale refreshes fail nonzero.
- Kept `status.recommendations.run` directly executable by removing uncached-count suffixes.
- Added spec coverage and regressions; all 43 graph conformance tests, all 40 status tests, and the final `make check` passed.
- Installed `tsift 0.1.96` locally; all 26 crate package checks and the npm publish dry run passed.
- Pushed commits `688b112` and `27cb0ac`, tag `v0.1.96`, and published GitHub binaries, crates.io packages, and `opencode-tsift@0.1.96`.
- Main CI and all seven Release workflow jobs passed; GitHub issue #24 is closed and no open issues remain.
- Preserved the original dirty `src/tsift` checkout and performed the release work in an isolated clean worktree.

### Re: Response

Fixed and released as agent-doc **0.35.315** / JetBrains plugin **0.2.371**.

- Root cause: provisional registration reset retry state before retained-projection reconciliation committed, causing one-second register/deregister/pull storms.
- Fix: only committed endpoints clear backoff; ambiguity now backs off to 30 seconds. See [CrdtReplicaManager.kt](/home/brian/work/btakita/agent-loop/src/agent-doc/editors/jetbrains/src/main/kotlin/com/github/btakita/agentdoc/CrdtReplicaManager.kt:2523).
- Regression coverage added at [CrdtReplicaForwarderTest.kt](/home/brian/work/btakita/agent-loop/src/agent-doc/editors/jetbrains/src/test/kotlin/com/github/btakita/agentdoc/CrdtReplicaForwarderTest.kt:518) and [RetainedCanonicalOperatorTextTest.kt](/home/brian/work/btakita/agent-loop/src/agent-doc/editors/jetbrains/src/test/kotlin/com/github/btakita/agentdoc/RetainedCanonicalOperatorTextTest.kt:151).
- `make check` and all 503 JetBrains tests pass.
- Commits pushed, tag `v0.35.315` pushed, release CI observed queued. Controllers are fresh and stable.
- No reactive topology extraction was needed: controller event ingress and the keyed retry scheduler remain authoritative; the defect was the premature state transition.

I did not capture a usable JVM stack—the restart happened before JVM attach completed—but the logs provided definitive causal evidence, including the same live storm recurring on `contracts.md`.

**Restart IDEA once more now.** Plugin 0.2.371 is installed, but PID `3019171` is still running the old loaded Kotlin classes.
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
