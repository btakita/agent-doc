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

Added prioritized FlowCore refactor next steps. Top backlog item: #rte4.
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7a9b103bd5b68223dac05a4c449ac03899aba53d6022e82100578b5b0-20260520-154344.md`*

Compacted content:
- Archived 4 response topic(s): routed-reopen FlowCore slice; FlowCore closeout, mutation, and session-cycle slices; duplicate content corruption during typing; 1 more
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7...
❯ Please elaborate on the decision in [#gp9d].
### Re: #gp9d audit-docs freshness decision — gpt-5.5

[#gp9d] is about separating two different kinds of doc-audit guarantees that currently look like one failure.

The useful guarantee is content freshness for managed instruction surfaces: generated agent-doc surfaces such as root `AGENTS.md`, `.codex/AGENTS.md`, `.claude/skills/agent-doc/SKILL.md`, and OpenCode/Cursor mirrors should match what the running binary would render. That is deterministic and worth keeping fail-closed because a stale harness instruction can make every later agent-doc cycle unsafe.

The questionable guarantee is broad mtime freshness for local prose docs. In the last cycle, `make check`, build, install, the JetBrains plugin build, and CI/tmux all passed, but `cargo run --quiet -- audit-docs` stayed red because several instruction/doc files were older than `src/stream.rs`. That does not prove those files contain stale content; it proves only that the source file has a newer filesystem timestamp. After source-only changes, that can create a release-blocking audit failure even when no human-facing instruction text needed to change.

The decision in [#gp9d] is therefore whether `audit-docs` should continue treating mtime as a hard correctness signal, or whether it should switch the managed instruction-surface check to content-based comparison and reserve mtime checks for narrower cases.

My recommendation is content-based for generated/managed surfaces, with optional advisory mtime output for broad docs. The managed-surface path already has a clear expected rendering from the binary, so it can fail on real content drift. For README/SPEC/custom instruction files, mtime is too indirect as a hard gate; a better gate is either explicit required-reference coverage, changed-file-aware checks, or targeted assertions that a spec section was updated when a known code area changed.

I would keep [#gp9d] open until that policy is made explicit in `SPEC.md` and covered with tests. The important invariant is not "every doc must be newer than every touched source file"; it is "agent-facing instructions and required specs must not drift from the binary behavior they claim to describe."
❯ do [#gp9d] with the recommended content based strategy. I am still questioning why the document itself is still being watched long after a turn ends. Could you both improve the content-based reconciliation logic and tell me why agent-doc still manages a document after the turn is complete. Is this useful for realtime documents?
### Re: #gp9d audit-docs content freshness — gpt-5

Implemented and shipped the content-based audit-docs strategy.

What changed:

- `src/audit_docs.rs` now keeps generated agent-doc instruction surfaces release-blocking through rendered-content comparison, then runs the rest of the agent-doc audit locally so generic mtime staleness is converted into non-blocking `Mtime advisory` rows.
- Blocking audit-docs failures remain content/structure based: managed surface content mismatch, tree path issues, actionable-content issues, context-invariant issues, and line budget failures.
- `tests/test_cli.rs` now covers the source-newer-than-doc case and asserts that it succeeds with advisory output instead of failing.
- `SPEC.md`, `README.md`, and `specs/07-core-commands.md` now state the policy explicitly: filesystem mtime freshness is advisory for agent-doc audits; generated instruction surfaces fail only when rendered content differs.

Why the document is still managed after a turn:

The document is not only a prompt payload; it is the durable session UI. After a response commits, agent-doc still keeps enough state around to support later edits, route/sync ownership, CRDT sidecars, snapshots, repair, and optional watch/reactive workflows. That is useful for realtime documents because the next user edit can be detected and reconciled against the last committed boundary without starting from scratch or losing editor state.

The important distinction is that this post-turn management should be lightweight ownership/sync state, not an open response cycle. A completed turn should be `committed`; later watches or syncs exist so the document remains a live collaboration surface. They should not imply that the previous turn is still active.

Verification:

- `cargo test audit_docs --test test_cli` passed.
- `make check` passed after the final source change.
- `agent-doc audit-docs` now passes with mtime-only drift reported as advisory output.
- `make install` completed, and the installed binary reports `agent-doc 0.33.16`.
- Latest GitHub Actions `ci.yml` run on `20260422/codex-agent-backend` is green, including the `Run make tmux-ci` step.

Commits pushed:

- `src/agent-doc`: `1a0aa26d` — `fix: make audit-docs mtime advisory`
- outer `agent-loop`: `7d3d421bb` — `chore: update agent-doc submodule`

I left the pre-existing dirty `CLAUDE.md` typechange and untracked `AGENTS.md` inside `src/agent-doc` untouched.
<!-- agent:boundary:5fbb042c -->
<!-- /agent:exchange -->
###
<!--
-->

<!-- agent:queue -->
dispatch #spec-test-build-install-commit-push
<!-- /agent:queue -->

## Pending / Not Built

<!-- agent:backlog -->
- [ ] [#nh3j] [recommended] Clarify audit-docs --root behavior for submodule-local managed skill artifacts; the no-override release path audits the superproject install root, while an explicit submodule root can still see stale tracked .claude/skills/agent-doc/SKILL.md artifacts.
- [ ] [#ops2] [recommended] Move operator_clear and orchestration_batch onto FlowCore typed outcomes after route and closeout extraction; cover active-pane clear refusal, queue freeze/mid-run mutation, and child patchback normalization. Plan: tasks/agent-doc/plan-agent-doc-flowcore-refactor.md
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
