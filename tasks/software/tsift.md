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

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc65443c45f7b49f5e6ace54e3f6bafe4077ade57924422b5eb3baf4dbd23c-20260506-073336.md`*

Icebox:
- [ ] [#9jc8] graphify integration: LLM-based entity extraction for semantic layer beyond AST
- [ ] [#zv4m] graphify integration: HTML visualization of symbol/dependency graphs
- [ ] [#rd2f] graphify integration: semantic similarity between concepts (embedding-based)
❯ What are #next-steps for tsift to save tokens while maintaining the appropriate amount of context & a good user experience?
### Re: next steps for tsift token efficiency — gpt-5

The highest-leverage next step is to make tsift budget-aware by default rather than relying on callers to remember `--compact`, `--terse`, `--tabular`, `diff-digest`, or `session-review --next-context` manually.

1. `#ts1a` Add budget-aware response profiles for `search`, `explain`, and `session-review`: explicit max-items/max-bytes modes with stable ids for follow-up expansion.
2. `#ts1b` Add a single agent-facing context-pack command that composes existing bounded surfaces (`diff-digest`, `test-digest`, `log-digest`, `session-review --next-context`) into one resumable handoff payload.
3. `#ts1c` Add MCP/tool-native structured envelopes for terse results so clients can render summaries and request expansion without paying prose tokens up front.
4. `#ts1d` Improve lock/degraded-mode UX: when a scoped index writer is active, prefer read-only/exact fallback or a minimal actionable summary instead of pushing the user into a noisy retry path.

On the RTK question: yes, it can help if the harness can keep large artifacts out-of-band, but it is a secondary win. The primary win is still source-side shaping: less output, grouped output, and expansion on demand. Backlog top: `#ts1a`.
<!-- agent:boundary:c03caccb -->
<!-- /agent:exchange -->
###

<!--
What are #next-steps for tsift to save tokens while maintaining the appropriate amount of context & a good user experience?
---
Can tsift use rtk to compress tool call output in the context?
-->

## Pending / Not Built

<!-- agent:backlog -->
- [ ] [#ts1a] Add budget-aware response profiles for tsift search/explain/session-review with max-items or max-bytes limits and stable expansion handles
- [ ] [#ts1b] Add a single agent-facing context-pack command that composes diff-digest, test-digest, log-digest, and session-review --next-context
- [ ] [#ts1c] Add MCP/tool-native structured terse envelopes so clients can render summaries and request follow-up expansion without prose-heavy defaults
- [ ] [#ts1d] Improve tsift lock/degraded-mode UX so active index writers fall back to read-only or exact search with a minimal actionable summary
<!-- /agent:backlog -->

<!-- agent:icebox -->
- [ ] [#9jc8] graphify integration: LLM-based entity extraction for semantic layer beyond AST
- [ ] [#zv4m] graphify integration: HTML visualization of symbol/dependency graphs
- [ ] [#rd2f] graphify integration: semantic similarity between concepts (embedding-based)
<!-- /agent:icebox -->

<!--
-->
