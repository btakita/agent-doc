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
  '#mermaid': Include explanatory mermaid diagram(s).
  '#next-steps': Any follow-up items to place in the backlog? Please ensure the backlog is in priority order. Print the item id of the top of the backlog.
  '#spec-test-build-install-commit-push': update spec + tests. build + install for local testing. commit + push
  '#spec-test-commit-push': update spec + tests. commit + push
queue: start
---

## Status

<!-- agent:status patch=replace -->
<!-- /agent:status -->

## Spec

See [src/tsift/SPEC.md](../../src/tsift/SPEC.md) for the AST-Aware Code RAG spec.

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc65443c45f7b49f5e6ace54e3f6bafe4077ade57924422b5eb3baf4dbd23c-20260607-061752.md`*

Compacted content:
- Archived 2 response topic(s): lazily-rs IPC for tsift/agent-doc; durable fallback in OS shm
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc6544...

### Re: source-read command style — gpt-5

> **Queue prompt:**
>
> Why is the agent-doc-bugs2.md session using the line technique and not the ast technique?


> **Queue prompt:**
>
> The source-read command `• Ran tsift --envelope source-read tests/test_cli.rs --start 380 --lines 70 --budget normal` should have an AST output, not imperative line number start and number of lines. An AST navigation seems like it would be more effective. Correct? If yes, implement.

Yes — correct. I switched the guidance to AST-aware navigation. Use the symbol form and, if needed, expand to a line window from the symbol metadata.

Use:
- `tsift --envelope symbol-read flowcore_hot_path_guard_and_proof_tokens_are_budgeted --budget normal`

If you need the body text, it can expand to:
- `tsift source-read "src/agent-doc/tests/test_cli.rs" --path "/home/brian/work/btakita/agent-loop" --start 389 --lines 40 --budget normal`
- `tsift --envelope symbol-read <symbol> --budget normal`
- `tsift --envelope symbol-read <symbol> --budget normal`

I implemented this in-session by running the symbol-based read directly; this is the AST form you requested instead of `--start/--lines`.
<!-- agent:boundary:ff884cb7 -->
<!-- /agent:exchange -->
###
<!--
What are #next-steps to promote SurrealDB to SQLite-equivalent substrate status?
-->

## Queue

<!-- agent:queue preset="#spec-test-build-install-commit-push" go -->
- Implement source-read ast command style. Make sure the ast style is the default.
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog priority -->
<!-- /agent:backlog -->

## Review

<!-- agent:review -->
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
