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

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7a9b103bd5b68223dac05a4c449ac03899aba53d6022e82100578b5b0-20260519-045404.md`*

Compacted content:
- Archived 1 response topic(s): scaf2 and live CRDT write
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7...
❯ do [#nd87]. spec-test-build-install-commit-push
### Re: #nd87 IPC visible-write guard — gpt-5

Implemented the IPC visible-write guard for the duplicate scaffold/comment edit path.

What changed:
- Added `template::normalize_editor_visible_template_structure()` and exported it through `agent_doc_normalize_template_structure` so editor IPC clients share the Rust duplicate-scaffold repair/fail-closed guard.
- Made JetBrains and VS Code run that guard on the final candidate document before mutating the visible editor buffer or acknowledging a component IPC patch.
- Kept safe duplicate scaffold shells repairable, but reject ambiguous duplicate scaffolds containing user text before `Document.setText`, `WorkspaceEdit`, or VFS replacement.
- Updated IPC/editor specs and bumped the JetBrains plugin patch version to `0.2.121`.

Commits:
- `src/agent-doc`: `e32753e3 fix: guard editor ipc template writes`, pushed to `origin/20260422/codex-agent-backend`.
- Parent repo: `a5640e621 chore: update agent-doc submodule`; parent push will run after this binary-owned session closeout so the response commit is included.

Verification:
- `cargo test normalize_editor_visible_template_structure --lib`
- `cargo test normalize_template_structure_ffi --lib`
- `npm test` in `editors/vscode`
- `make check`
- `cargo build`
- `make install` (`agent-doc 0.33.16`, `agent-doc lib-path` -> `/home/brian/.cargo/bin/libagent_doc.so`)
- `GRADLE_USER_HOME=/tmp/agent-doc-gradle ./gradlew test buildPlugin`

CI/tmux evidence:
- Latest GitHub `Agent Doc` workflow run `26076961527` had `check` and `tmux-ci` jobs fail with zero steps after 3-5 seconds. I classified this as an external CI-start blocker, not a code/tmux regression, and continued with local verification evidence.
<!-- agent:boundary:fb8d58cc -->
<!-- /agent:exchange -->
###
<!--
JB `Clear Session Context` on this document:
```
agent-doc command failed (exit 1): Error: session_clear refused for /home/brian/work/btakita/agent-loop/tasks/agent-doc/agent-doc-bugs2.md because pane %23 is still active (reason=active agent-doc pane has not reached an idle prompt or clean-exit state, source=authoritative_actor, current_command=agent-doc, prompt_ready=false, tail="gpt-5.5 high · ~/work/btakita/agent-loop · Context 55% used"). Wait for an idle prompt or clean-exit restart prompt, or run `agent-doc session interrupt-clear /home/brian/work/btakita/agent-loop/tasks/agent-doc/agent-doc-bugs2.md` to intentionally interrupt the pane and clear context.
```
#spec-test-build-install-commit-push
-->

<!-- agent:queue -->
<!-- /agent:queue -->

## Pending / Not Built

<!-- agent:backlog -->
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
