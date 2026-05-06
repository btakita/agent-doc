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

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc65443c45f7b49f5e6ace54e3f6bafe4077ade57924422b5eb3baf4dbd23c-20260506-051802.md`*

Icebox:
- [ ] [#9jc8] graphify integration: LLM-based entity extraction for semantic layer beyond AST
- [ ] [#zv4m] graphify integration: HTML visualization of symbol/dependency graphs
- [ ] [#rd2f] graphify integration: semantic similarity between concepts (embedding-based)
❯ Please look through the Claude Code + Codex session history. What are the #next-steps for tsift to save tokens?
### Re: tsift token next steps — gpt-5

I checked the archived tsift token-savings investigation from April 27-28 against the current `src/tsift/CLAUDE.md`, `src/tsift/VERSIONS.md`, and a fresh `tsift session-review tasks/software/tsift.md` run.

Most of the original backlog is already shipped: search timeout, quiet `index --check`, `--compact`, compact JSON by default, relative paths, `--limit`, `--terse`, `--schema`, Codex autoindex, and the digest surfaces (`diff-digest`, `log-digest`, `test-digest`, `session-digest`, `session-cost`, `session-review`). So the next token wins have shifted.

Current priority order:

1. **#srevmatch** — tighten `session-review` matching. This run had to consider 3,812 sessions and matched 12 largely because the workspace `cwd` is broad, so the aggregate counts are useful directionally but too noisy for per-doc review. The next step is stricter path/session-alias matching plus bounded discovery (`--strict`, `--since`, `--max-sessions`, or similar).
2. **#digestpref** — make the digest commands the default workflow in both Claude and Codex instructions. The toolchain now has the right surfaces, but the history still shows raw transcript/diff/log reads and some plain `rg` usage slipping through. `tsift init` should inject guidance for `session-review`, `session-digest`, `diff-digest`, `log-digest`, and `test-digest`, not just search/graph/explain.
3. **#budgetguard** — turn session-cost warnings into actionable guardrails. This review reported a 96.56% cached-input ratio, 2,322 no-op closeouts, and restart churn (`fresh_restart`, `auto_trigger_timeout`, `ctrl_d_restart_loop`). That should become a machine-readable “compact/restart/split now” signal instead of something you only notice after a huge session.

Lower priority after those: dogfood `tsift summarize` on `src/tsift` and re-measure. The earlier “~78% savings” claim was still theoretical at the time of the archive because summaries had not actually been populated.

Top backlog item: `#srevmatch`.
<!-- agent:boundary:ac1365ed -->
<!-- /agent:exchange -->
###

<!--
Can tsift use rtk to compress tool call output in the context?
-->

## Pending / Not Built

<!-- agent:backlog -->
- [ ] [#9m98] srevmatch=Tighten tsift session-review matching for document-targeted reviews so shared cwd matches do not overcount unrelated Claude/Codex sessions.
- [ ] [#5zm8] digestpref=Expand tsift init / code-navigation guidance so Claude and Codex prefer session/diff/log/test digest commands over raw transcript, git diff/show, and verbose log reads.
- [ ] [#0nv1] budgetguard=Add session-cost/session-review budget guardrails for cached-context resend, repeated no-op closeouts, and restart churn, with hook-friendly thresholds and remediation hints.
<!-- /agent:backlog -->

<!-- agent:icebox -->
- [ ] [#9jc8] graphify integration: LLM-based entity extraction for semantic layer beyond AST
- [ ] [#zv4m] graphify integration: HTML visualization of symbol/dependency graphs
- [ ] [#rd2f] graphify integration: semantic similarity between concepts (embedding-based)
<!-- /agent:icebox -->

<!--
-->
