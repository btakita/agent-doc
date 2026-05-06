---
agent_doc_session: tsift-v0.1
resume: 019dd526-7958-77e3-9a41-92b8944fc550
agent: codex
claude_model: claude-opus-4-6
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

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc65443c45f7b49f5e6ace54e3f6bafe4077ade57924422b5eb3baf4dbd23c-20260506-063004.md`*

Active backlog:
- [ ] [#ctxpack] Add a resumable context-pack / session-review next-context output that carries only active prompt targets, last verification state, touched files/symbols, unresolved failures, and next digest commands.
- [ ] [#shmatch] Tighten cross-harness session-review matching/parsing for Claude JSONL path/session aliases and malformed-content lines so Claude + Codex history is comparable.
- [ ] [#looprank] Add duplicate-turn and loop clustering to session-cost/session-review so repeated prompt bodies, closeout churn, and repeated command bundles are summarized explicitly.
- 1 more active item(s)

Icebox:
- [ ] [#9jc8] graphify integration: LLM-based entity extraction for semantic layer beyond AST
- [ ] [#zv4m] graphify integration: HTML visualization of symbol/dependency graphs
- [ ] [#rd2f] graphify integration: semantic similarity between concepts (embedding-based)
❯ do [#ctxpack]. spec-test-build-install-commit-push
### Re: next-context pack — gpt-5

Added `tsift session-review --next-context`, which emits a bounded resumable handoff pack with active prompt targets, last verification state, touched files/symbols, unresolved failures, and the next digest commands. The full JSON `session-review` report now also carries `next_context`, and the human-readable `--next-context` mode prints only that pack.

Updated the spec, command docs, and versioned guidance to cover the new surface, and bumped `tsift` to `0.1.34`. Added unit and CLI integration coverage for both the aggregate JSON report and the dedicated `--next-context` output.

Verification in `src/tsift`: `cargo test session_review -- --nocapture`, `make check`, `cargo build --release`, `cargo install --path . --force`
Committed and pushed in `src/tsift` as `9016c12` (`feat: add session-review next-context pack`).
Committed in `agent-loop` as `a315688a7` (`chore: update tsift submodule`).
<!-- agent:boundary:a8a2f8e8 -->
<!-- /agent:exchange -->
###

<!--
What are #next-steps for tsift to save tokens while maintaining the appropriate amount of context & a good user experience?
---
Can tsift use rtk to compress tool call output in the context?
-->

## Pending / Not Built

<!-- agent:backlog -->
- [ ] [#shmatch] Tighten cross-harness session-review matching/parsing for Claude JSONL path/session aliases and malformed-content lines so Claude + Codex history is comparable.
- [ ] [#looprank] Add duplicate-turn and loop clustering to session-cost/session-review so repeated prompt bodies, closeout churn, and repeated command bundles are summarized explicitly.
- [ ] [#hookcaps] Extend digest-first rewrite/output capping beyond the current Claude RTK path so Codex and other harnesses also avoid verbose raw command/transcript context.
<!-- /agent:backlog -->

<!-- agent:icebox -->
- [ ] [#9jc8] graphify integration: LLM-based entity extraction for semantic layer beyond AST
- [ ] [#zv4m] graphify integration: HTML visualization of symbol/dependency graphs
- [ ] [#rd2f] graphify integration: semantic similarity between concepts (embedding-based)
<!-- /agent:icebox -->

<!--
-->
