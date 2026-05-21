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

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7a9b103bd5b68223dac05a4c449ac03899aba53d6022e82100578b5b0-20260521-173011.md`*

Compacted content:
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7...
❯ The duplicate content corrupting document and duplicate prompt issues happened yet again. Very tired of playing whack-a-mole. Reproduce bugs with tests first that fail and fix the implementation. Instead of yet another edge case fix (for all we know the edge cases are infinite), create a structural fix to eliminate this class of errors. If we need to remove features, we can talk about it. #spec-test-build-install-commit-push
### Re: duplicate prompt corruption — gpt-5

I reproduced the failure first and fixed it structurally rather than adding another duplicate-tail special case.

The underlying problem was that `preflight` could still mutate the visible session document before it had a stable view of the same content it later reported through `baseline_file` and diff. In this case, preflight logged duplicate prompt cleanup, but the live document could still retain the duplicate HTML comment because the cleanup happened while the shared editor state was not settled. That left the response cycle split between a cleaned baseline snapshot and a still-corrupt visible document.

The fix changes `preflight` so document-mutating recovery, pending maintenance, commit recovery, and duplicate-residue cleanup all wait for the shared typing indicator to go idle first. If the editor never settles within the bounded wait, preflight fails closed instead of writing into a moving document. The baseline file is now saved from the exact stable visible-document content used to compute the diff, eliminating the split-brain path that produced repeated duplicate prompts.

Changed paths in `src/agent-doc`:

- `src/preflight.rs`
- `src/diff.rs`
- `specs/07-closeout-commands.md`
- `specs/07-core-commands.md`
- `VERSIONS.md`

Regression proof:

- Added `preflight_waits_for_typing_before_duplicate_prompt_comment_cleanup`; it failed before the implementation change and passes after it.
- Kept the existing duplicate-comment regressions passing: `preflight_removes_post_exchange_duplicate_prompt_comment_before_diff` and `normalize_final_template_content_removes_duplicate_prompt_html_comment_tail`.

Verification run:

- `cargo fmt --manifest-path src/agent-doc/Cargo.toml`
- `cargo test preflight_waits_for_typing_before_duplicate_prompt_comment_cleanup --manifest-path src/agent-doc/Cargo.toml`
- `cargo test preflight_removes_post_exchange_duplicate_prompt_comment_before_diff --manifest-path src/agent-doc/Cargo.toml`
- `cargo test normalize_final_template_content_removes_duplicate_prompt_html_comment_tail --manifest-path src/agent-doc/Cargo.toml`
- `git -C src/agent-doc diff --check`
- `make check` in `src/agent-doc`
- `make install` in `src/agent-doc`
- `agent-doc --version` reports `agent-doc 0.33.16`
- GitHub CI run `26243096548` passed, including both `make check` and `make tmux-ci`.

Committed and pushed:

- `src/agent-doc`: `9a5479da` (`fix: debounce preflight baseline capture`) pushed to `origin/20260422/codex-agent-backend`.
- Superproject: `67809e59e` (`agent-doc: debounce preflight baseline capture`) pushed to `origin/main`.

No feature removal was needed.
<!-- agent:boundary:48784fd3 -->
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
