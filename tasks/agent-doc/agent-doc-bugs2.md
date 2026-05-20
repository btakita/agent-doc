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

FlowCore refactor is complete enough to leave broad extraction mode; next top backlog item: #fcrg.
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7a9b103bd5b68223dac05a4c449ac03899aba53d6022e82100578b5b0-20260520-223000.md`*

Compacted content:
- Archived 1 response topic(s): auto queue continuation
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7...
The duplicate content corrupting document and duplicate prompt issues happened yet again. Very tired of playing whack-a-mole. Reproduce bugs with tests first that fail and fix the implementation. #spec-test-build-install-commit-push
### Re: duplicate scaffold repair — gpt-5

Reproduced the mixed duplicate-scaffold closeout as a failing regression first in `normalize_template_structure_repairs_duplicate_scaffold_with_user_text`; it failed before the fix because safe prompt text stranded between duplicate `agent:exchange` close markers was treated as ambiguous escaped conversation content.

Fixed the normalizer to recognize the duplicated scaffold plus safe prompt-text shape, drop the duplicated `queue`/`backlog`/`done` scaffold, move the prompt text back into `agent:exchange`, and keep truly ambiguous suffixes fail-closed. Wired the repair through binary closeout normalization, editor-visible normalization, and FFI, and updated `SPEC.md`, `specs/07-closeout-commands.md`, and `VERSIONS.md`. The FlowCore guard budget was audited and incremented by one because this new repair path reruns the existing conversation-tail guard after normalization.

Verification:
- failing-first: `cargo test normalize_template_structure_repairs_duplicate_scaffold_with_user_text` failed before the implementation, then passed after the fix
- focused: `cargo test duplicate_scaffold`; `cargo test duplicate_exchange_close`; `cargo test live_prompt_prefix_variant`; `cargo test normalize_final_template_content`; `cargo test flowcore_hot_path_guard_and_proof_tokens_are_budgeted --test test_cli`
- full: `make check` passed after the FlowCore budget audit
- build/install: `cargo build` passed; `make install` passed and installed `agent-doc 0.33.16`
- tmux/CI: latest GitHub `CI` run `26192346037` succeeded with `Run make check` and `Run make tmux-ci`; local `make tmux-ci` passed with 228 ignored/live tests

Commits created:
- `src/agent-doc`: `a1bd5840 fix: repair mixed duplicate scaffold closeouts`
- parent repo: `82f0bc912 fix: update agent-doc duplicate scaffold repair`

Push will run immediately after this document closeout so the session-document commit is included on `origin/main`.
<!-- agent:boundary:905257b5 -->
<!-- /agent:exchange -->
###
<!--
The duplicate content corrupting document and duplicate prompt issues happened yet again. Very tired of playing whack-a-mole. Reproduce bugs with tests first that fail and fix the implementation. #spec-test-build-install-commit-push 
-->

<!-- agent:queue -->
<!-- /agent:queue -->

## Pending / Not Built

<!-- agent:backlog -->
- [ ] [#pdgd] pending_done_guard misclassifies modify-vs-complete: (a) flags items rescoped via --pending-edit/--pending-gate as needing --done (false positive), (b) misses items that are genuinely executed when their IDs only appear in response body section headers (false negative). Observed 2026-05-20 on session-share/tasks/root.md cycle 'do [#arsq] [#axid] [#rprd]' — guard warned on #fvtg/#qew8/#p635 (just rescoped) but said nothing about the three actually-completed items. Plan: tasks/agent-doc/plan-pending-done-guard-completion-detection.md
- [ ] [#y9kk] [recommended] Phase 1 for lower-agent orchestration: define job packet + worker result schemas, add the manual job-packet runbook, and include examples for search, test triage, small patch, and spec update jobs. Plan: tasks/agent-doc/plan-lower-agent-orchestration-token-savings.md
- [ ] [#v44k] [recommended] Phase 2 for lower-agent orchestration: add tsift JSON contracts for context-pack, source-read, diff-digest, test-digest, index-status, stable handles, freshness checks, and token estimates. Plan: tasks/agent-doc/plan-lower-agent-orchestration-token-savings.md
- [ ] [#ffsm] [recommended] Phase 3 for lower-agent orchestration: extend agent-doc plan with task_class, dispatch_candidate, model tier, context budget, write scope, required proof, and tsift missing/stale fallback behavior. Plan: tasks/agent-doc/plan-lower-agent-orchestration-token-savings.md
- [ ] [#rafe] [recommended] Phase 4 for lower-agent orchestration: implement agent-doc jobs create/list/status/collect plus optional operation-doc generation and closeout links. Plan: tasks/agent-doc/plan-lower-agent-orchestration-token-savings.md
- [ ] [#ek86] Implement the lower-agent orchestration MVP from tasks/agent-doc/plan-lower-agent-orchestration-token-savings.md, starting with manual job packets plus tsift context packets.
- [ ] [#fwjw] Define the agent-doc taxonomy for agent docs, job packets, operation docs, runbooks, and plans.
- [ ] [#0fcx] Specify operation docs/opdocs as retained audit artifacts and how they relate to job packets.
<!-- /agent:backlog -->

<!-- agent:icebox -->
<!-- /agent:icebox -->

## Completed / Reaped

<!-- agent:done archive=tasks/agent-doc-bugs2.done.md -->

<!-- completed work archived in tasks/agent-doc-bugs2.done.md -->
<!-- /agent:done -->
