---
agent_doc_session: tsift-v0.1
resume:
  claude: 7ec8a9b0-c6b3-46ca-b057-dfbb94db6d6d
  codex: 01a093e4-2a62-7d70-902a-b29046babb50
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

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc65443c45f7b49f5e6ace54e3f6bafe4077ade57924422b5eb3baf4dbd23c-20260911-150553.md`*

Compacted content:
- Archived 8 response topic(s): Session resumed; Issue #25 fixed and v0.1.97 published; tsift compared with Graft; 5 more
- Prior summary/context: prior compacted content: Archived 6 response topic(s): tsift 0.1.96 released; Response; do [#fixcompactexchange]; 3 more
> **Queue prompt:** fpe.md had a tsift task running for 1hr 51min before I killed it. Fix it.

### Re: fpe.md runaway tsift task — gpt-5 · 2026-09-12T00:37-04:00

Fixed. This was an `agent-doc` dispatch-lifecycle race, not a tsift execution bug: a `Ready` projection consumed the in-flight receipt before the dispatched turn actually started, so the idle watcher admitted a duplicate long-lived task into the same composer.

The exact `fpe.md` trace was already fixed, committed, and pushed as agent-doc commit `b666a8d7` (`fix(dispatch): prove a dispatch's turn started instead of inferring it from Ready`). Dispatch receipts now move to `running` only after an observed `Busy` transition from `caller=dispatch`; `Ready` consumes only a proven-running receipt, with a bounded 120-second grace path for genuinely lost triggers. Unrelated supervisor `Busy` transitions cannot vouch for a dispatch.

The installed `agent-doc 0.35.367` binary was built at 00:06, after the fix commit at 23:35. I did not reinstall over the unrelated concurrent `0.35.368` source edits.

Verification is green:

- exact state-store regression for the 33-second premature-Ready race;
- lost-trigger grace regression;
- controller lifecycle/coalescing regression;
- structural Busy/Ready wiring regression;
- latest GitHub Actions CI run `34672029496` completed successfully.

No additional source changes were necessary in this cycle; all three fix-owned paths are clean.

### Re: agent-doc commit and push check — gpt-5 · 2026-09-12T00:40-04:00

> **Queue prompt:**
>
> do [#changecommitpush]


I made no `agent-doc` source change, so there is no new change of mine to commit or push. The `fpe.md` fix is already committed as `b666a8d7` and present on `origin/main`.

The three fix-owned paths remain clean. Local `agent-doc` `main` is currently one commit ahead at `8912cc44` with additional dirty editor files, but that commit and those edits were created concurrently by another session; I did not push or fold them into this task.
<!-- agent:boundary:d4c17bb5:tsift -->
<!-- /agent:exchange -->

## Queue

<!-- agent:queue preset="#spec-test-build-install-commit-push" priority -->
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
