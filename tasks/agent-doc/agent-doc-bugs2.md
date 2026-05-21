---
agent_doc_session: b26b9957-8de6-4eae-bb98-5d7fe4d6b781
agent: codex
claude_model: opus
agent_doc_format: template
agent_doc_write: crdt
claude_args: --dangerously-skip-permissions
codex_args: -s danger-full-access
prompt_presets:
  '#agent-doc-bug': Please create a plan for agent-doc to fix this issue. Add to the backlog of tasks/agent-doc/agent-doc-bugs2.md
  '#code-review': Please review agent-doc codebase. Inspect the architecture and look for edge cases. Isolate concurrency issues and other workflow bugs. Let's find root causes to fix.
  '#commit-push': commit + push all uncommitted files
  '#follow-up-backlog': Any follow-up items to place in the backlog?
  '#next-steps': Any follow-up items to place in the backlog? Please ensure the backlog is in priority order. Print the item id of the top of the backlog.
  '#spec-test-build-install-commit-push': update spec + tests. build + install for local testing. commit + push
  '#spec-test-commit-push': update spec + tests. commit + push
queue_active: false
---

## Status

<!-- agent:status patch=replace -->

Added auto-DAG next steps; top backlog item: #autodag.
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7a9b103bd5b68223dac05a4c449ac03899aba53d6022e82100578b5b0-20260521-144508.md`*

Compacted content:
- Archived 2 response topic(s): jobslock; jobslock
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7...
❯ The duplicate content corrupting document and duplicate prompt issues happened yet again. Very tired of playing whack-a-mole. Why is it so difficult to recognize that I'm typing in a prompt? I clearly see the diff in my editor. Reproduce bugs with tests first that fail and fix the implementation. #spec-test-build-install-commit-push
### Re: duplicate prompt corruption — gpt-5

You were right about the shape: the prompt diff was visible and preflight did recognize it, but the newer comment-preservation rule was too broad. It preserved a stale post-exchange HTML comment containing an older copy of the same prompt, so the document could keep carrying duplicate prompt residue even though the active prompt was already in `agent:exchange`.

I reproduced that first with failing regressions, starting with `cargo test duplicate_prompt_html_comment --lib`, which failed because editor-visible normalization kept the duplicate comment. I then fixed the shared cleanup path so preflight, editor-visible normalization, IPC snapshot dedupe, and final template reconciliation remove ordinary post-exchange HTML comments only when the body duplicates or near-duplicates a prompt already present in `agent:exchange`; unrelated scratch comments still stay outside the exchange.

Changed paths in `src/agent-doc`: `src/template.rs`, `src/preflight.rs`, `src/write.rs`, `tests/finalize_integration.rs`, `specs/04-diff-computation.md`, `specs/07-closeout-commands.md`, and `VERSIONS.md`.

Verification completed:
- Failing-first proof: `cargo test duplicate_prompt_html_comment --lib` failed before implementation.
- Focused passes: `cargo test duplicate_prompt_html`, `cargo test post_exchange_duplicate`, and `cargo test html_comment_tail`.
- CI/tmux evidence: latest `agent-doc` CI run `26233010436` was green, including `Run make tmux-ci`.
- Full local suite: `make check` passed.
- Local install: `make install` passed and `agent-doc --version` reports `agent-doc 0.33.16`.

Commits:
- `src/agent-doc`: `0bc3c58` (`fix: clean duplicate prompt comments`), pushed to `origin/20260422/codex-agent-backend`.
- Superproject pointer: `d6c744949` (`agent-doc: clean duplicate prompt comments`), ready to push after this document closeout commit.
<!-- agent:boundary:af702f7a -->
<!-- /agent:exchange -->
###



<!-- agent:queue -->
preset #spec-test-build-install-commit-push
- do [#autodag]
- do [#dagdispatch]
- do [#dagrecover]
- do [#daglogs]
<!-- /agent:queue -->

## Pending / Not Built

<!-- agent:backlog -->
- [ ] [#autodag] [recommended] Add an agent-doc auto-DAG planner for queued do/compound directives: parse explicit depends-on text, backlog ids, graph/conflict evidence, and job packets into a topological schedule; fail closed on cycles, missing ids, stale graph evidence, or ambiguous ownership before dispatch.
- [ ] [#dagdispatch] [recommended] Execute auto-DAG batches through agent-doc orchestrate/jobs: run parallel-safe antichain batches, collect worker_result proof, unblock downstream nodes only after required proof/session-check passes, and stop deterministically on failed/gated nodes without launching dependents.
- [ ] [#dagrecover] [recommended] Add auto-DAG persistence/recovery semantics: write operation docs/job packets with schedule id, node state, attempt count, replay commands, and repair commands so interrupted DAG runs can resume idempotently after compact/finalize/restart.
- [ ] [#daglogs] [recommended] Use session-review guardrails as scheduler input: classify prompt-budget/cache-resend/restart-loop/noop-closeout log families before auto-DAG start and gate large or looping runs into compact/restart/fixture-fix steps.
<!-- /agent:backlog -->

<!-- agent:icebox -->
<!-- /agent:icebox -->

## Completed / Reaped

<!-- agent:done archive=tasks/agent-doc-bugs2.done.md -->

<!-- completed work archived in tasks/agent-doc-bugs2.done.md -->
<!-- /agent:done -->
