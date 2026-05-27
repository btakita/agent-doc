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
queue_active: true
---

## Status

<!-- agent:status patch=replace -->
OpenCode tsift shortcuts shipped and pushed. #gopencode is gated on the remaining live agent-doc OpenCode smoke; #gbackendproof is complete.
<!-- /agent:status -->

## Spec

See [src/tsift/SPEC.md](../../src/tsift/SPEC.md) for the AST-Aware Code RAG spec.

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc65443c45f7b49f5e6ace54e3f6bafe4077ade57924422b5eb3baf4dbd23c-20260527-200555.md`*

Compacted content:
- Archived 7 response topic(s): tokensave vs tsift; OpenCode integration next steps; OpenCode integration next steps; 4 more
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc6544...
do [#gopencode-install]. #spec-test-build-install-commit-push
<!-- agent:boundary:ca565911 -->
<!-- /agent:exchange -->
###
<!--
What are #next-steps to make the graph faster?
---
What are #next-steps to make the graph + neighborhoods faster + more effective?
-->

<!-- agent:queue auto -->
- do [#gopencode-install]. #spec-test-build-install-commit-push
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog -->
- [ ] [#gopencode-install] Make tsift installable as an opencode harness via `tsift init --opencode`: publish the command shortcuts to the opencode marketplace/registry so users can install tsift opencode support without cloning the repo, document the install path, and verify end-to-end with a fresh opencode session.
<!-- /agent:backlog -->

## Review

<!-- agent:review -->
- [/] [#gopencode] [tag: opencode] [tag: integration] Make OpenCode a first-class tsift integration: add installer/config docs, envelope shortcuts for session/log/test/diff digest use, verify OpenCode transcript capture and permission-prompt diagnostics, add deterministic tests, then build/install and run an end-to-end agent-doc OpenCode smoke session.
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
