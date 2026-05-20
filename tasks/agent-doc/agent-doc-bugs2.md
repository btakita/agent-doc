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

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7a9b103bd5b68223dac05a4c449ac03899aba53d6022e82100578b5b0-20260520-173312.md`*

Compacted content:
- Archived 1 response topic(s): live typing corruption and fan-out orchestration
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7...
❯ do [#ewzz]. #spec-test-build-install-commit-push
### Re: editor IPC apply proof — gpt-5

Completed `#ewzz`.

Implemented editor-side apply proof for both plugins. VS Code now captures the exact buffer text plus `TextDocument.version` after debounce and re-checks it before full-content, component-patch, and boundary-reposition mutations. JetBrains now captures the exact document text plus `Document.modificationStamp` and re-checks before each visible `setText` path. Stale proof mismatches reject the patch without ACK, and socket IPC `status:error` ACKs now fail the send path so the CLI can retry or fall back instead of treating a rejected editor mutation as delivered.

Updated the shared editor specs, bumped local plugin builds to JetBrains `0.2.125` and VS Code `0.2.19`, and added focused VS Code, JetBrains, and socket-ACK regressions. Committed the submodule change as `a0e4dcde fix: guard editor IPC apply generation` and the parent gitlink as `d174cc13b agent-doc: guard editor IPC apply generation`; the push follows this closeout boundary.

Verification: `cargo test socket_error_ack_is_error`; `npm test -- --test-name-pattern patchGuard`; `npm test`; `./gradlew test --tests com.github.btakita.agentdoc.NormalizeExchangePrefixesTest`; `./gradlew test buildPlugin`; `make check`; `make install`; installed `agent-doc 0.33.16`. Latest checked CI tmux evidence was run `26178914295`, where `Run make tmux-ci` succeeded. I left unrelated pre-existing local edits in `src/agent-doc` unstaged.
<!-- agent:boundary:0eeed238 -->
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
