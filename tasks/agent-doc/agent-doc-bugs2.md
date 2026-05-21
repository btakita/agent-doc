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

FlowCore refactor is in maintenance mode; top backlog item: #ipcd.
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7a9b103bd5b68223dac05a4c449ac03899aba53d6022e82100578b5b0-20260521-043857.md`*

Compacted content:
- Archived 1 response topic(s): lower-agent orchestration
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7...
### Re: review backlog lifecycle — gpt-5

Implemented #rvel, #fwjw, and #0fcx.

The pending system now has a first-class `agent:review` component. `--pending-gate` moves backlog items into review, `--pending-ungate` moves them back to backlog, `agent-doc migrate` moves legacy `[/]` backlog items into review, and preflight reports `review_count` / `review_gated_count` plus a warning for legacy gated backlog entries. I also added `--review-add` and `--review-edit` for direct review-list mutations, and `review_done_guard` as an opt-in closeout guard that can require gated review before `--done` completes a tracked item.

I updated the docs and specs to describe the new review lifecycle, the agent-doc / job packet / opdoc / runbook / plan taxonomy, and the retained-audit role of opdocs. The FlowCore guard budget was updated only after adding a typed closeout reason for strict `review_done_guard` blocks.

Verification completed:

- `cargo test --test pending_integration`
- `cargo test component::`
- `cargo test migrate::`
- `cargo test frontmatter::`
- `cargo test --test finalize_integration`
- `cargo test skill::`
- `cargo test --test test_cli flowcore_hot_path_guard_and_proof_tokens_are_budgeted`
- `cargo fmt --check`
- `git diff --check`
- `make check`
- `cargo build`
- `cargo install --path .`
- GitHub CI run `26205740633` was green; its `Run make tmux-ci` step completed successfully at `2026-05-21T04:46:44Z`.

Committed implementation in `src/agent-doc` as `171f7939 feat: add review backlog lifecycle`, then committed the parent submodule pointer as `843d1c8c4 agent-doc: update review lifecycle`.
<!-- agent:boundary:132fec51 -->
<!-- /agent:exchange -->
###
<!--
The duplicate content corrupting document and duplicate prompt issues happened yet again. Very tired of playing whack-a-mole. Reproduce bugs with tests first that fail and fix the implementation. #spec-test-build-install-commit-push
-->


<!-- agent:queue -->
preset #spec-test-build-install-commit-push
- do [#rvel]
- do [#fwjw]
- do [#0fcx]
<!-- /agent:queue -->

## Pending / Not Built

<!-- agent:backlog -->
- [ ] [#aggtp] Add an agent-doc graph-backed orchestration acceptance gate that dogfoods the latest tsift graph-db evidence, conflict-matrix, dispatch-trace, worker_result, and job packet contracts against agent-doc jobs create/collect; fail closed when packet ids, projection hashes, replay/repair commands, touched files/tests, or follow-up ids are missing.
<!-- /agent:backlog -->

<!-- agent:icebox -->
<!-- /agent:icebox -->

## Completed / Reaped

<!-- agent:done archive=tasks/agent-doc-bugs2.done.md -->

<!-- completed work archived in tasks/agent-doc-bugs2.done.md -->
<!-- /agent:done -->
