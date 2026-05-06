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

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc65443c45f7b49f5e6ace54e3f6bafe4077ade57924422b5eb3baf4dbd23c-20260506-034301.md`*

Active backlog:
- [ ] [#sbudget] Add token budget and churn guardrails to session-cost: flag turns by prompt size, cached-input ratio, restart loops, and no-op closeout cycles with actionable compact/restart guidance.
- [ ] [#snoopdedupe] Collapse repeated committed/no-op closeout events in agent-doc log digests so long sessions report unique cycles instead of hundreds of commit_already_current repeats.

Icebox:
- [ ] [#9jc8] graphify integration: LLM-based entity extraction for semantic layer beyond AST
- [ ] [#zv4m] graphify integration: HTML visualization of symbol/dependency graphs
- [ ] [#rd2f] graphify integration: semantic similarity between concepts (embedding-based)
❯ do [#sbudget]. spec-test-build-install-commit-push
### Re: `[#sbudget]` session-cost guardrails — gpt-5

Implemented `#sbudget` in `src/tsift` by adding structured `guardrails` to `session-cost` and aggregate `session-review` output. The new guardrails explicitly flag oversized prompt turns, very high cached-input resend ratios, restart-loop churn, and repeated `commit_already_current` no-op closeouts, each with actionable compact/restart guidance. I also surfaced those guardrails in both compact and normal CLI output, updated `SPEC.md`, and added unit plus CLI JSON regression coverage.

Verification:
- `cargo test`
- `cargo build --release`
- `cargo install --path . --force`
- `tsift session-review --compact ../../tasks/software/tsift.md`

Local commits:
- `src/tsift`: `accca07` (`feat: add session cost guardrails`)
- `agent-loop`: `a18d69557` (`chore: bump tsift submodule`)

Note: preflight warned that this document has recent closeout churn and suggested `agent-doc compact /home/brian/work/btakita/agent-loop/tasks/software/tsift.md --commit` before another large turn.
❯ do [#snoopdedupe]. spec-test-build-install-commit-push
### Re: `[#snoopdedupe]` session digest closeout dedupe — gpt-5

Implemented `#snoopdedupe` in `src/tsift` by normalizing newer `agent-doc` `document_cycle event=...` log lines into distinct runtime events and counting them once per `(cycle, event)` instead of replaying every repeated `commit_already_current` closeout line. `session-cost` now reports deduped `commit_success` / `commit_already_current` runtime events directly, and `session-digest` collapses closeout evidence to bounded `document_cycle phase=committed event=...` entries instead of one entry per `cycle=` / `capture_id=` string.

I also updated the `Session Cost` / `Session Review` spec text and added regression coverage for the new `document_cycle` log shape.

Verification:
- `cargo test`
- `cargo build --release`
- `cargo install --path . --force`
- `tsift session-cost --json --source agent-doc-log --input /home/brian/work/btakita/agent-loop/.agent-doc/logs/tsift-v0.1.log`
- `tsift session-digest --json --source agent-doc-log --path /home/brian/work/btakita/agent-loop/tasks/software/tsift.md --input /home/brian/work/btakita/agent-loop/.agent-doc/logs/tsift-v0.1.log`
- `tsift session-review --json /home/brian/work/btakita/agent-loop/tasks/software/tsift.md`

Live check: the real `tsift-v0.1` agent-doc log now reports `commit_success` and `commit_already_current` as separate runtime events, and the closeout digest for that log is bounded to normalized event types (`closeout_groups: 9`) instead of hundreds of raw per-cycle no-op entries.

Local commits:
- `src/tsift`: `1a8b763` (`fix: dedupe agent-doc closeout cycles in session digests`)
- `agent-loop`: `df4ca2779` (`chore: bump tsift submodule`)
<!-- agent:boundary:e4191b93 -->
<!-- /agent:exchange -->
###

<!--
Can tsift use rtk to compress tool call output in the context?
-->

## Pending / Not Built

<!-- agent:backlog -->
<!-- /agent:backlog -->

<!-- agent:icebox -->
- [ ] [#9jc8] graphify integration: LLM-based entity extraction for semantic layer beyond AST
- [ ] [#zv4m] graphify integration: HTML visualization of symbol/dependency graphs
- [ ] [#rd2f] graphify integration: semantic similarity between concepts (embedding-based)
<!-- /agent:icebox -->

<!--
-->
