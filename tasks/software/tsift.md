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

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc65443c45f7b49f5e6ace54e3f6bafe4077ade57924422b5eb3baf4dbd23c-20260506-024713.md`*

Icebox:
- [ ] [#9jc8] graphify integration: LLM-based entity extraction for semantic layer beyond AST
- [ ] [#zv4m] graphify integration: HTML visualization of symbol/dependency graphs
- [ ] [#rd2f] graphify integration: semantic similarity between concepts (embedding-based)
❯ Please review Claude Code + Codex session logs. What are #next-steps for tsift to save tokens?
### Re: tsift token-saving next steps — gpt-5

I reviewed representative exact-match Claude and Codex transcripts for `tasks/software/tsift.md`, plus the `agent-doc` runtime log for this document.

Evidence:
- Claude long session `fcf730fe-7580-494c-bbf8-b3175c90350a.jsonl`: about `3.68M` total tokens, `97.66%` cached input, largest turns around `94k-96k` tokens.
- Claude shorter follow-up `47de3be5-52c7-41c7-af5f-5f63874a08db.jsonl`: about `109k` total tokens, with a `39k`-token peak turn.
- Codex long session `rollout-2026-05-05T16-50-50-019df9e8-6687-7332-92d0-5e4fd2916ac2.jsonl`: about `25.13M` total tokens, `98.41%` cached input, largest turns around `237k-245k` tokens.
- Codex shorter follow-up `rollout-2026-05-05T22-45-44-019dfb2d-52c9-7693-8b02-c010b52663a9.jsonl`: about `892k` total tokens, with a `150k`-token peak turn.
- The `agent-doc` runtime log for this doc shows `62` session starts, `28` fresh-restart events, `8` Ctrl-D restart-loop events, and `5` auto-trigger timeouts.

Conclusion: the dominant cost is context accretion plus session churn, not assistant output length. The biggest savings are in making tsift better at finding the right logs, collapsing them into one bounded report, filtering copied instruction ballast, and warning when a session is mostly paying to resend cached context.

I added these next steps to backlog in priority order:
1. `#srevagg` Build document-scoped cross-harness session review so "review logs for this task" becomes one bounded tsift command instead of manual file hunting plus per-file digests.
2. `#sfilter` Filter instruction ballast out of digest/cost summaries. In the Claude digest, prompt targets were mostly headings from copied harness instructions, which dilutes the signal.
3. `#sbudget` Add token-budget and churn guardrails to `session-cost`, especially for huge cached-input turns and restart-heavy sessions.
4. `#snoopdedupe` Collapse repeated no-op closeout events in `agent-doc` log summaries so long sessions report unique cycles instead of commit noise.

Top backlog item: `#srevagg`.
<!-- agent:boundary:f09c2ce4 -->
<!-- /agent:exchange -->
###

<!--
Can tsift use rtk to compress tool call output in the context?
-->

## Pending / Not Built

<!-- agent:backlog -->
- [ ] [#srevagg] Build document-scoped cross-harness session review: auto-discover related Claude/Codex/agent-doc logs for a target doc or repo path and emit one aggregated cost + digest report.
- [ ] [#sfilter] Filter instruction ballast from session-digest/session-cost so prompt targets and hotspots stop treating copied harness instructions as real user work.
- [ ] [#sbudget] Add token budget and churn guardrails to session-cost: flag turns by prompt size, cached-input ratio, restart loops, and no-op closeout cycles with actionable compact/restart guidance.
- [ ] [#snoopdedupe] Collapse repeated committed/no-op closeout events in agent-doc log digests so long sessions report unique cycles instead of hundreds of commit_already_current repeats.
<!-- /agent:backlog -->

<!-- agent:icebox -->
- [ ] [#9jc8] graphify integration: LLM-based entity extraction for semantic layer beyond AST
- [ ] [#zv4m] graphify integration: HTML visualization of symbol/dependency graphs
- [ ] [#rd2f] graphify integration: semantic similarity between concepts (embedding-based)
<!-- /agent:icebox -->

<!--
-->
