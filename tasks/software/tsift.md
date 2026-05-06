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

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc65443c45f7b49f5e6ace54e3f6bafe4077ade57924422b5eb3baf4dbd23c-20260506-055406.md`*

Active backlog:
- [ ] [#5zm8] digestpref=Expand tsift init / code-navigation guidance so Claude and Codex prefer session/diff/log/test digest commands over raw transcript, git diff/show, and verbose log reads.
- [ ] [#0nv1] budgetguard=Add session-cost/session-review budget guardrails for cached-context resend, repeated no-op closeouts, and restart churn, with hook-friendly thresholds and remediation hints.

Icebox:
- [ ] [#9jc8] graphify integration: LLM-based entity extraction for semantic layer beyond AST
- [ ] [#zv4m] graphify integration: HTML visualization of symbol/dependency graphs
- [ ] [#rd2f] graphify integration: semantic similarity between concepts (embedding-based)
❯ do [#5zm8]. spec-test-build-install-commit-push
### Re: #5zm8 digest guidance — gpt-5

Completed `#5zm8`.

- Expanded the `tsift init` Code Navigation block so Claude/Codex now prefer bounded digest surfaces: `session-digest` / `session-review`, `diff-digest`, `test-digest`, and `log-digest`, while keeping the owning-root/submodule-root guidance.
- Mirrored that guidance in `SPEC.md`, `CLAUDE.md`, and the generated `AGENTS.md`, and added init regression coverage in `src/init.rs`.
- `make check` passed after fixing a strict clippy lint in `src/session_review.rs`.
- Local install completed with `cargo install --path .`.
- Repo commit + push completed in `src/tsift`: `5c48cff` (`docs: prefer digest surfaces in init guidance`).
<!-- agent:boundary:e1aa30ce -->
<!-- /agent:exchange -->
###

<!--
Can tsift use rtk to compress tool call output in the context?
-->

## Pending / Not Built

<!-- agent:backlog -->
- [ ] [#0nv1] budgetguard=Add session-cost/session-review budget guardrails for cached-context resend, repeated no-op closeouts, and restart churn, with hook-friendly thresholds and remediation hints.
<!-- /agent:backlog -->

<!-- agent:icebox -->
- [ ] [#9jc8] graphify integration: LLM-based entity extraction for semantic layer beyond AST
- [ ] [#zv4m] graphify integration: HTML visualization of symbol/dependency graphs
- [ ] [#rd2f] graphify integration: semantic similarity between concepts (embedding-based)
<!-- /agent:icebox -->

<!--
-->
