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

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7a9b103bd5b68223dac05a4c449ac03899aba53d6022e82100578b5b0-20260521-222359.md`*

Compacted content:
- Archived 1 response topic(s): #missedpb missed patchback closeout
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7...
❯ The duplicate content corrupting document and duplicate prompt issues happened yet again. Very tired of playing whack-a-mole. Reproduce bugs with tests first that fail and fix the implementation.

❯ Please review all fixes of "duplicate content corrupting document and duplicate prompt issues". Evaluate any refactoring opportunities. Evaluate if the entire class of bug can be eliminated with different solution.

❯ #spec-test-build-install-commit-push
### Re: duplicate prompt corruption review — gpt-5

Root cause: the earlier fixes covered preflight, final template reconciliation, IPC snapshot dedupe, and editor-visible normalization, but `agent-doc route` still dispatched against the saved document before running that shared duplicate prompt-comment cleanup. That left the scratch comment body under `agent:exchange` visibly dirty until preflight cleaned it, which is why this kept feeling like whack-a-mole even after the closeout/write paths were protected.

I reproduced the missing boundary with a failing regression first: `cargo test route_scrubs_duplicate_prompt_comment_before_dispatch --manifest-path src/agent-doc/Cargo.toml` initially failed because the route layer had no pre-dispatch duplicate-comment scrub. The fix wires route to the existing `template::remove_post_exchange_duplicate_prompt_comments` scanner, preserving the empty ordinary HTML comment shell and unrelated scratch comments while removing only duplicate prompt text before a routed reopen is sent.

Review of the prior fixes: the debounce/preflight baseline fix is still necessary because it prevents split-brain baselines; the comment-shell preservation fix is still necessary because the shell is user-owned scratch structure; the missing piece was that route/submit needed to share the same cleanup before dispatch, not wait for the next preflight. This is the refactoring direction I used: no second scanner, one shared template cleanup called from every binary-owned boundary that can see this shape.

Changed paths:
- `src/agent-doc/src/route.rs`
- `src/agent-doc/src/route/tests.rs`
- `src/agent-doc/SPEC.md`
- `src/agent-doc/specs/07-closeout-commands.md`
- `src/agent-doc/VERSIONS.md`

Verification:
- Failing-first proof: `cargo test route_scrubs_duplicate_prompt_comment_before_dispatch --manifest-path src/agent-doc/Cargo.toml` failed before the route cleanup was implemented, then passed after the fix.
- `cargo test duplicate_prompt_comment --manifest-path src/agent-doc/Cargo.toml`
- `cargo test scrubs_duplicate_prompt_html_comment_body --manifest-path src/agent-doc/Cargo.toml`
- `cargo fmt --manifest-path src/agent-doc/Cargo.toml`
- `git -C src/agent-doc diff --check`
- `make check` in `src/agent-doc`: passed, including 2242 default tests and the wider deterministic closeout simulation.
- `make install` in `src/agent-doc`: passed; `agent-doc --version` reports `agent-doc 0.33.16`.
- `make tmux-ci` in `src/agent-doc`: passed, 228 live tmux tests.
- Latest upstream `agent-doc` CI run `26256391346` was green before this commit, including the `Run make tmux-ci` step.

Commit / push:
- `src/agent-doc`: `0270b51d` (`fix: scrub duplicate prompt comments before route`) pushed to `origin/20260422/codex-agent-backend`.
- Root repo: `c29de70be` (`agent-doc: scrub duplicate prompt comments before route`) is committed locally and will be pushed to `origin/main` immediately after this binary-owned session closeout commit lands, per the session-document staging rule.

This does not mathematically eliminate duplicate prompt text from arbitrary manual Markdown editing, but it closes the class for the supported binary-owned boundaries: route, preflight, editor-visible normalization, IPC snapshot dedupe, and final write reconciliation now share the same duplicate post-exchange prompt-comment scrub while preserving user scratch comments.
<!-- agent:boundary:c8e62150 -->
<!-- /agent:exchange -->
###
<!--
-->

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
