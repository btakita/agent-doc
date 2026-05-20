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
Completed stale visible-closeout write guard and added the IPC editor-side proof follow-up. Top backlog item: #ipcw.
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7a9b103bd5b68223dac05a4c449ac03899aba53d6022e82100578b5b0-20260520-171724.md`*

Compacted content:
- Archived 3 response topic(s): #gp9d audit-docs freshness decision; #gp9d audit-docs content freshness; commit-push src/agent-doc instructions
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7...
❯ The content-based implementation still does not fix the duplicate content corrupted document & duplicate typing line issues. As I was typing into the comment below agent:exchange in this document, the issues surfaced.
❯ #spec-test-build-install-commit-push
❯ ---
❯ How does this Claude code tip reconcile with our orchestration implementation?
```
  ⎿  Tip: Say "fan out subagents" and Claude sends a team. Each one digs deep so nothing gets missed.
```
❯ #next-steps
### Re: live typing corruption and fan-out orchestration — gpt-5

Implemented the stale visible-write guard for the remaining live-typing race.

The content-based repairs were catching some already-corrupted shapes, but the write path still trusted a merge that had been computed against `content_current` even if the visible markdown changed again before snapshot/document mutation. That is exactly the danger window when a user is typing in an exchange-adjacent scratch comment or exchange tail. The fix is now fail-closed: template/CRDT disk writes, IPC timeout disk fallbacks, and repair replays re-read the document after the active-typing guard and refuse to write if it no longer matches the content used for the merge. The refusal logs `visible_write_current_changed:<source>` and leaves the new user edit visible for the next cycle instead of committing a stale merge.

Changed and pushed in `src/agent-doc`:
- `6677f821` `fix: block stale visible closeout writes`

Parent repo pushed:
- `93bf9440d` `agent-doc: block stale visible closeout writes`

Verification passed:
- `cargo test visible_write_guard`
- `make check`
- latest GitHub CI run `26176939086` was green, including `Run make tmux-ci`
- `cargo build --release`
- `cargo install --path . --force`
- `cargo build --release --lib`
- `agent-doc lib-install`
- `agent-doc --version` -> `agent-doc 0.33.16`

On the Claude Code tip: it is a product-level natural-language affordance, not a closeout contract. In agent-doc terms, "fan out subagents" should reconcile to an explicit orchestration plan: bounded jobs, declared write scopes, dispatch proof, result collection, and one binary-owned parent closeout. Agent-doc should not let hidden harness-native fanout write the session document independently, because that would bypass the same finalize/session-check boundary this workflow depends on. The existing lower-agent orchestration backlog is the right direction for token-saving fanout; the new top follow-up is narrower and more urgent for this bug family: add editor-side IPC apply proof so stale editor buffers cannot apply append/full-content patches after the binary has already protected the disk path.

Top backlog item: `#ipcw`.
<!-- agent:boundary:e6541873 -->
<!-- /agent:exchange -->
###
<!--
-->

<!-- agent:queue -->
dispatch #spec-test-build-install-commit-push
<!-- /agent:queue -->

## Pending / Not Built

<!-- agent:backlog -->
- [ ] [#ewzz] #ipcw [recommended] Add editor-side IPC apply proof that rejects stale buffer or generation mismatches before applying exchange append patches while user typing; cover file IPC, socket IPC, and full-content repair paths. Plan: tasks/agent-doc/plan-live-typing-duplicate-insertion.md
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
