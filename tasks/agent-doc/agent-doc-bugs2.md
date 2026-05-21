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

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7a9b103bd5b68223dac05a4c449ac03899aba53d6022e82100578b5b0-20260521-061501.md`*

Compacted content:
- Archived 3 response topic(s): queued auto-DAG work; queued auto-DAG retry; tsift backlog capture
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7...
❯ The duplicate content corrupting document and duplicate prompt issues happened yet again. Very tired of playing whack-a-mole. Reproduce bugs with tests first that fail and fix the implementation. #spec-test-build-install-commit-push
### Re: duplicate prompt/editor normalization — gpt-5

Reproduced the duplicate-prompt corruption with a failing test first. The uncovered path was editor-visible template normalization: `normalize_editor_visible_template_structure` did not remove ordinary post-exchange HTML comments whose body duplicated the current prompt, even though preflight/final template cleanup had related repairs.

Changed paths:

- `src/agent-doc/src/template.rs`
- `src/agent-doc/src/pending.rs`
- `src/agent-doc/specs/07-closeout-commands.md`

Implementation:

- Added `normalize_editor_visible_template_structure_removes_duplicate_prompt_comment`, which failed before the fix because the duplicate post-exchange prompt comment remained outside `agent:exchange`.
- Routed editor-visible normalization through duplicate prompt-comment cleanup, limited to ordinary non-agent HTML comments after `agent:exchange` whose body duplicates or near-duplicates prompt candidates already present in the exchange tail. Unrelated scratch comments and tracked-work scaffold remain intact.
- Updated the closeout spec to require this invariant for editor-visible template normalization as well as final reconciliation.
- Inspected the latest completed CI run, which was red on Rust 1.95 clippy for `src/pending.rs:908` (`explicit_counter_loop`) before reaching tmux. Fixed that loop and added `op_append_items_preserves_order_after_existing_items` to cover the order-preserving behavior.

Verification passed locally on the final tree:

- Red-first: `cargo test normalize_editor_visible_template_structure_removes_duplicate_prompt_comment -- --nocapture` failed before the implementation and passed after.
- Focused checks: `cargo test normalize_editor_visible_template_structure -- --nocapture`, `cargo test duplicate_prompt_html_comment -- --nocapture`, `cargo test preflight_removes_duplicate_post_exchange_prompt_comment_before_diff -- --nocapture`, `cargo test op_append_items_preserves_order_after_existing_items -- --nocapture`.
- Full checks: `cargo fmt --check`, `git diff --check`, `cargo test` (`2233 passed; 0 failed; 228 ignored`), `make check`, `cargo build`, `cargo install --path . --force`, `agent-doc --version` (`agent-doc 0.33.16`).
- Tmux proof: latest completed remote CI before this fix was run `26207660510` on `9f06a014`, failed in `make check`, and skipped `make tmux-ci`; local `make tmux-ci` passed on this final tree (`228 passed; 0 failed`).
- Remote CI for pushed commit `9ee8c629` started as run `26209899896` and was still in progress at closeout, with `make check` running and `make tmux-ci` pending.

Commit/push state:

- Submodule commit `9ee8c62` (`fix: clean editor duplicate prompt comments`) was pushed to `btakita/agent-doc` branch `20260422/codex-agent-backend`.
- The superproject `src/agent-doc` gitlink already points at `9ee8c629` on `origin/main` via concurrent commit `b850ed2`; my explicit root pointer commit attempt found no remaining staged gitlink diff, so I did not create a duplicate root commit.
- This session response is being finalized through `agent-doc finalize`; I will push `main` after finalize so the session closeout commit is remote as well.
<!-- agent:boundary:fc1774f3 -->
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
- [ ] [#jobslock] `agent-doc jobs create` fails when `tsift graph-db status` returns the SQLite hot-journal/read-only error (recurrence of `#w25k`). Observed 2026-05-21 second-cycle `#dogfood-tsift` run: `agent-doc jobs create tasks/software/tsift.md --audit` exited with `plan has blocker(s); refusing to create jobs: tsift graph evidence failed closed: ... Error code 5: The database file is locked`. Fix on the agent-doc side: when `tsift graph-db` is locked, fall back to a soft warning + `manual_packet_only: true` flag in the planning record rather than refusing job creation entirely. tsift-side root-cause fix lives at `tasks/software/tsift.md` `#gdblock`.
<!-- /agent:backlog -->

<!-- agent:icebox -->
<!-- /agent:icebox -->

## Completed / Reaped

<!-- agent:done archive=tasks/agent-doc-bugs2.done.md -->

<!-- completed work archived in tasks/agent-doc-bugs2.done.md -->
<!-- /agent:done -->
