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

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7a9b103bd5b68223dac05a4c449ac03899aba53d6022e82100578b5b0-20260520-205405.md`*

Compacted content:
- Archived 2 response topic(s): FlowCore refactoring next steps; FlowCore regression gate
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7...
❯ As I was typing in this prompt, the duplicate content corrupt document & the duplicate prompt bugs surfaced again. Please, dear God, make this architecturally, fundamentally, by default logic robust. I'm tired of playing whack-a-mole with these bugs. Your current approach of edge case fixing is simply not working. #spec-test-build-install-commit-push

❯ What are the #next-steps for flow core?
### Re: full-content IPC guard and FlowCore next step — gpt-5

I replaced the fragile part with a default invariant: any whole-document editor IPC replacement now has to prove the source buffer identity before the editor is allowed to mutate visible text. The binary stamps `fullContent` socket/file payloads with a `patch_id`, expected source-buffer hash, and byte length; JetBrains and VS Code reject the replacement without ACK if the live editor/VFS buffer differs; and the binary refuses to save a snapshot if a consumed full-content patch leaves anything other than the requested target document.

That covers the dedicated `try_ipc_full_content` compact/repair path and the older `try_ipc` normalization fallback path that could also send `fullContent`. I updated the spec and changelog, bumped local plugin versions to JetBrains `0.2.127` and VS Code `0.2.20`, and added Rust, JetBrains, and VS Code tests for the source-buffer proof.

Verification completed: targeted Rust payload test, VS Code `npm test -- --runInBand`, JetBrains `GRADLE_USER_HOME=/tmp/agent-doc-gradle ./gradlew test buildPlugin`, full `make check`, `make build-release`, `make install`, VS Code VSIX packaging/install, and JetBrains local install into the active IntelliJ IDEA `2026.1` profile. The latest GitHub CI run I inspected also has `make check` and `make tmux-ci` green.

Code commits: submodule `8889122f` (`fix: guard full-content editor ipc`) is already pushed, and the root gitlink commit `0ddf4a6f` is ready; I will push the root after this binary-owned document closeout so the session commit is included too.

For FlowCore, the next step I recorded in backlog is to make this proof a shared `flow::document_mutation` visible-replacement contract rather than an IPC-specific convention, with simulator coverage for stale editor buffers across compact, repair, and IPC timeout recovery.
<!-- agent:boundary:8cf809ab -->
<!-- /agent:exchange -->
###
<!--
-->

<!-- agent:queue -->
<!-- /agent:queue -->

## Pending / Not Built

<!-- agent:backlog -->
- [ ] [#bqn4] [recommended] FlowCore Phase 2: move full-content source-buffer proof into flow::document_mutation as the shared visible-replacement contract, with simulator coverage for stale editor buffers during compact, repair, and IPC timeout recovery.
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
