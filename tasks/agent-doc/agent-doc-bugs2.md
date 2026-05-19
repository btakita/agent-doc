---
agent_doc_session: b26b9957-8de6-4eae-bb98-5d7fe4d6b781
agent: codex
claude_model: claude-opus-4-6
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

Reviewed recent agent-doc logs; top backlog item: #rtbr.
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7a9b103bd5b68223dac05a4c449ac03899aba53d6022e82100578b5b0-20260519-023545.md`*

Compacted content:
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7...
❯ Review logs for agent-doc bugs. #next-steps
### Re: log review / next steps — gpt-5

Reviewed the recent agent-doc logs for this document. The active signal is still startup routing: the latest ops summary shows four `route_authoritative_actor_starting_not_ready` entries for `tasks/agent-doc/agent-doc-bugs2.md` generation 182, after an auto-started Codex pane stayed in `starting` past the 8s route window.

I added `#rtbr` as the new top backlog item, tied to the existing `tasks/agent-doc/plan-current-tmux-route-sync-next-steps.md` plan. That keeps the priority on replacing fixed startup sleep budgets with a current-generation ready-prompt barrier or one typed timeout.

I did not add a separate item for the single `post_commit_local_drift kind=working_tree_edits` / `commit_noop drift_kind=working_tree_edits` entry because this cycle deliberately changed the prompt tail and removed the old queue entries; it is worth watching, but the route timeout recurrence is the clearer actionable bug.

Top backlog item: `#rtbr`.
❯ do [#rtbr]. #spec-test-build-install-commit-push
### Re: rtbr — gpt-5
Implemented `#rtbr`.

The route now records starting-actor timeouts per document, pane, and actor generation. The first timeout for a stuck generation still writes the typed `route_authoritative_actor_starting_not_ready` diagnostic; later reroutes against the same pane/generation write `route_starting_actor_timeout_coalesced` instead. The timeout sentinel clears when that generation reaches `ready`, `closed`, or `blocked`, so a later real state transition or new generation gets fresh diagnostics.

I added route-state unit coverage and SimWorld coverage for the repeated-starting-timeout schedule, and updated the README/spec/version notes for the new coalescing contract. While rerunning tmux CI, I also fixed two exposed guard issues: plain `register/register` actor projections no longer get treated as degraded managed authoritative actors, and the live raw tmux input test now accepts the CR byte sequence tmux emits for Enter in raw mode.

Verification completed:

- `make check` passed.
- `TEST_THREADS=8 TMUX_TEST_THREADS=8 make tmux-ci` passed: 228 live/ignored tests run.
- Focused regressions for starting timeout coalescing, SimWorld, degraded authoritative fallback, dispatch-only busy/startup-miss routing, and supervisor IPC tmux input all passed.
- `make install` completed and installed `agent-doc 0.33.16` plus `libagent_doc-0.33.16.so`.
- CI review: latest available GitHub CI run on `main` was successful (`25773430221`, 2026-05-13). No branch CI run exists for `20260422/codex-agent-backend`; local `make tmux-ci` is the tmux evidence for this turn.

Commits:

- `src/agent-doc`: `433f119b` (`fix: coalesce starting actor route timeouts`), pushed to `origin/20260422/codex-agent-backend`.
- superproject: `aa44cdf88` (`chore: update agent-doc submodule`) created with only the `src/agent-doc` pointer staged; I am leaving this document closeout to `agent-doc finalize` before pushing `main`.
<!-- agent:boundary:ce090356 -->
<!-- /agent:exchange -->
###
<!--
Use TEST_THREADS=8. spec-test-build-install-commit-push
-->

<!-- agent:queue -->
<!-- /agent:queue -->

## Pending / Not Built

<!-- agent:backlog -->
- [ ] [#scaf2] [recommended] Ship and verify the CRDT duplicate-scaffold closeout guard for live exchange typing: observed `tasks/software/agent-loop.md` duplicated `<!-- /agent:exchange -->`, copied pending/done scaffold into the exchange tail, and split live `agent-skill` / `agent-skills is` text after a 0.33.16 CRDT closeout. Plan: tasks/agent-doc/plan-crdt-duplicate-scaffold-closeout.md
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
