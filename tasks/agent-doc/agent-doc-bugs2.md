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

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7a9b103bd5b68223dac05a4c449ac03899aba53d6022e82100578b5b0-20260519-053118.md`*

Compacted content:
- Archived 1 response topic(s): JetBrains Clear Session Context default Codex placeholder
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7...
❯ JB `Compact Exchange` results in an external file edit. The CRDT should handle the compaction with no external file edit dialog. #spec-test-build-install-commit-push
### Re: Compact Exchange editor IPC and CI tmux visibility — gpt-5

Fixed the JetBrains Compact Exchange external-file-edit issue in `src/agent-doc`.

Root cause: Compact Exchange reused the response-fallback full-content IPC path. That path correctly rejects stale response repair after the prior cycle is already committed, but compact is an operator mutation, not late response repair. The rejection forced compact to fall back to a direct disk write, which is what caused JetBrains to show the external file edit dialog.

What changed:

- `src/write.rs` now separates full-content IPC into response fallback and operator mutation modes. The already-committed-cycle guard still protects response fallback, while operator mutation can use editor IPC after a committed prior cycle.
- `src/compact.rs` now applies compacted documents through the operator-mutation IPC path.
- Added regression coverage for Compact Exchange after a previous cycle is already committed, including proof that it emits/uses `fullContent` IPC and does not log `late_fallback_patch_rejected`.
- Updated `editors/SPEC.md` and `VERSIONS.md` with the expected Compact Exchange IPC behavior.
- Updated `.github/workflows/ci.yml` so GitHub Actions visibly shows separate `Run make check` and `Run make tmux-ci` steps, with a `Running make tmux-ci` log marker in the tmux step.

Verification completed:

- `cargo test component_compact`
- `cargo test full_content_ipc_skips_committed_cycle_before_socket_or_file_fallback`
- `make check`
- `make tmux-ci` (`228 passed; 0 failed` in the live tmux sweep)
- `make install`
- Inspected latest GitHub CI run `25773430221`: it was green, but its old workflow log did not visibly show a separate tmux leg. The workflow now will show `Run make tmux-ci` as its own step on the next CI run.

Committed and pushed in `src/agent-doc` on `20260422/codex-agent-backend`:

- `a4c0f3e5 fix: keep compact exchange on editor IPC`
- `3993a0b3 ci: name tmux integration step`

I left unrelated local/root worktree changes alone. The root superproject gitlink is still modified because `src/agent-doc` now points at the pushed submodule commits; I did not make a root superproject commit because the root worktree already has unrelated dirty files.
<!-- agent:boundary:ec67025e -->
<!-- /agent:exchange -->
###
<!--
Please address this issue on CI. CI should visibly demonstrate tmux-ci was run.
> but its visible log does not show a separate make tmux-ci leg

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
