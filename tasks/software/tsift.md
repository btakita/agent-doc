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
❯ - Repo commit + push completed in `src/tsift`: `5c48cff` (`docs: prefer digest surfaces in init guidance`).
❯ to [#0nv1]. spec-test-build-install-commit-push
### Re: #0nv1 budget guardrails — gpt-5

Completed `#0nv1` as a stale backlog closeout. The `session-cost` / `session-review` guardrails for cached-context resend, repeated `commit_already_current` no-op closeouts, and restart churn are already present in `src/tsift` from `accca07` (`feat: add session cost guardrails`), including CLI output, spec coverage, and regression tests.

- `make check` passed on the current `src/tsift` HEAD (`518` unit tests + `75` exit-code tests).
- `cargo install --path .` succeeded and refreshed the local `tsift` binary.
- `src/tsift` itself is clean on `main...origin/main`, so there was no repo delta to commit or push in this cycle.
❯ Look at the Claude Code + Codex session history. What are #next-steps for tsift to save tokens while maintaining the appropriate amount of context & a good user experience?
### Re: #next-steps for token-efficient context — gpt-5

The session history points to resend cost, not answer cost. `tsift session-review tasks/software/tsift.md` reported a `96.77%` cached-input ratio across `5.80B` total tokens, `2403` `commit_already_current` no-op closeouts, and restart churn on the document side. `tsift session-review src/tsift` showed the same repo pattern with a `96.86%` cached-input ratio and repeated `220K`-token turns with very small outputs. For this target, the matched history is effectively Codex-only; Claude transcripts were considered, but they did not match cleanly, which is itself a product gap.

What I’d put in the backlog, in priority order:

1. `#ctxpack` Add a resumable `context-pack` or `session-review --next-context` surface that emits the minimum carry-forward bundle: active prompt targets, last verified diff/test/install state, touched files/symbols, unresolved failures, and the recommended next digest commands. This is the most direct fix for the repeated `200K+` cached resend pattern.
2. `#shmatch` Tighten cross-harness session matching/parsing, especially Claude JSONL path/session alias detection and the noisy `did not contain message content or tool_use blocks` cases. Right now the cross-harness review is skewed because the `src/tsift` history matched `803` Codex sessions and `0` Claude sessions.
3. `#looprank` Add duplicate-turn and loop clustering to `session-cost` and `session-review`: identify repeated prompt bodies, repeated closeout cycles, and repeated command bundles like `cargo build`, `cargo install`, and `agent-doc finalize`, so the tool can say exactly what should be compacted or summarized away.
4. `#hookcaps` Extend digest-first rewriting/output capping beyond the current Claude `PreToolUse` RTK path. RTK already caps verbose `tsift` output for Claude, but the heavy-token sessions are still dominated by Codex prompt resend and repeated verbose command workflows.

Top backlog item: `#ctxpack`.

Operator note: this document already hit the session-accretion warning, so `agent-doc compact /home/brian/work/btakita/agent-loop/tasks/software/tsift.md --commit` is the right next session move before another long turn.
<!-- agent:boundary:fc8ee70c -->
<!-- /agent:exchange -->
###

<!--
What are #next-steps for tsift to save tokens while maintaining the appropriate amount of context & a good user experience?
---
Can tsift use rtk to compress tool call output in the context?
-->

## Pending / Not Built

<!-- agent:backlog -->
- [ ] [#ctxpack] Add a resumable context-pack / session-review next-context output that carries only active prompt targets, last verification state, touched files/symbols, unresolved failures, and next digest commands.
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
