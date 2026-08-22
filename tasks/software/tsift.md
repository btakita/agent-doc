---
agent_doc_session: tsift-v0.1
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

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc65443c45f7b49f5e6ace54e3f6bafe4077ade57924422b5eb3baf4dbd23c-20260822-020642.md`*

Compacted content:
- Archived 4 response topic(s): release + publish; How do we fix tsift-astgrep publishing?; reactive projection acceptance; 1 more
- Prior summary/context: prior compacted content: Archived 6 response topic(s): do [#registerextractfunction]; do [#extractfrommethod]; do [#e...

### Re: Fix tsift issues 1–3 — gpt-5

> **Queue prompt:**
>
> Fix https://github.com/btakita/tsift/issues/2


> **Queue prompt:**
>
> Fix https://github.com/btakita/tsift/issues/1


Implemented and committed all three issue fixes in the tsift submodule.

- **#1 — AWS Bedrock summarize:** extraction now resolves one reusable backend only for real cache misses. Hosted Claude providers run through authenticated, non-interactive `claude -p`; direct Anthropic keys retain the API transport. Missing or unusable credentials fail once, before the extraction walk, with a nonzero exit.
- **#2 — runbook location:** `.agent/runbooks/code-navigation.md` is canonical. `tsift init` migrates the legacy path while preserving unmanaged text, updates generated references, and is idempotent.
- **#3 — portable AGENTS boilerplate:** generated verification guidance is capability-based: `make check` or a supported `just` recipe, plus `gh` or `glab` only when the matching repository metadata and executable exist.

Verification is green: focused summarize/init/conformance tests, full `make check`, `cargo install --path .`, installed `tsift 0.1.80`, two-pass `tsift init .` idempotence, current `tsift status`, and the latest existing GitHub Actions CI run. The implementation commit is `2d018e2` (`Fixes #1`, `Fixes #2`, `Fixes #3`); its push follows this document closeout.
<!-- agent:boundary:cd881543 -->
<!-- /agent:exchange -->

## Queue

<!-- agent:queue preset="#spec-test-build-install-commit-push" priority go -->
- ~~Fix https://github.com/btakita/tsift/issues/1~~
- ~~Fix https://github.com/btakita/tsift/issues/2~~
- ~~Fix https://github.com/btakita/tsift/issues/~~
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
