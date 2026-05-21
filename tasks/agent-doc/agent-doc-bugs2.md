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
FlowCore refactor is in maintenance mode; top backlog item: #y9kk.
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7a9b103bd5b68223dac05a4c449ac03899aba53d6022e82100578b5b0-20260520-235644.md`*

Compacted content:
- Archived 1 response topic(s): do [#fcmt]
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7...
❯ JB `Interrupt and Clear Session Context` causes the Codex session to open nvim.
❯ #spec-test-build-install-commit-push
### Re: JB Interrupt and Clear Session Context — gpt-5

Implemented and pushed the fix for the JetBrains `Interrupt and Clear Session Context` path.

What changed:
- `agent-doc session interrupt-clear` now checks live pane evidence before sending interrupt keys. If the pane is already `alive-idle` with `prompt_ready=true`, it logs `session_interrupt_clear_skip_interrupt ... reason=already_idle`, reconciles stale projection if needed, and goes directly to the normal `/clear` path. Busy panes still take the explicit interrupt/discard path.
- Updated the JetBrains spec so the idle-prompt skip is part of the IDE action contract.
- Hardened the live tmux mock-agent launch helper because the first local tmux sweep exposed a drafted long `exec ... <absolute path>` launch under load.

Regression proof:
- Added Rust coverage for idle vs busy `interrupt-clear` initial action.
- Reproduced the bug first: `cargo test interrupt_clear_initial_action_skips_interrupt_keys_for_idle_pane` failed with `left: SendInterrupt`; after the fix, `cargo test interrupt_clear_initial_action` passed.

Verification:
- `make check` passed after the final edits.
- `/usr/bin/zsh -lc "GRADLE_USER_HOME=/tmp/agent-doc-gradle ./gradlew test buildPlugin"` passed.
- `make install` installed `agent-doc 0.33.16` and `libagent_doc-0.33.16.so` locally.
- `agent-doc plugin install --local jetbrains` installed `agent-doc-jetbrains-0.2.127.zip` to `/home/brian/.local/share/JetBrains/IntelliJIdea2026.1`.
- Latest completed GitHub CI before this push was green, including `Run make tmux-ci` (`26196808428`). The new post-push CI run is in progress (`26197772085`).
- Local `make tmux-ci` initially caught the mock-launch helper failure; after hardening the helper, the targeted live test passed and the full local `make tmux-ci` passed.

Commits:
- `src/agent-doc`: `5936228f fix: skip interrupt-clear keys on idle panes`, pushed to `origin/20260422/codex-agent-backend`.
- root superproject: `1875e6de3 chore: update agent-doc submodule`, pushed to `origin/main`.
<!-- agent:boundary:eec05bf0 -->
<!-- /agent:exchange -->
###
<!--
The duplicate content corrupting document and duplicate prompt issues happened yet again. Very tired of playing whack-a-mole. Reproduce bugs with tests first that fail and fix the implementation. #spec-test-build-install-commit-push 
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
