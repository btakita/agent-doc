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
  '#next-steps': ''
  '#spec-test-build-install-commit-push': update spec + tests. build + install for local testing. commit + push
  '#spec-test-commit-push': update spec + tests. commit + push
queue_active: false
---

## Status

<!-- agent:status patch=replace -->

Recorded Codex capability proof verification.
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7a9b103bd5b68223dac05a4c449ac03899aba53d6022e82100578b5b0-20260515-124309.md`*

Compacted content:
- Archived 5 response topic(s): queued spec/test/build/install; #writeempty spec/test/build/install; #startroute spec/test/build/install; 2 more
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7...
JB `/clear` on this document error:
```
agent-doc command failed (exit 1): Error: session_clear refused for /home/brian/work/btakita/agent-loop/tasks/agent-doc/agent-doc-bugs2.md because the authoritative actor is still starting and the document changed after the last committed cycle. Wait for a dispatch-ready prompt (`prompt_ready=true`) and retry, or run `agent-doc session status /home/brian/work/btakita/agent-loop/tasks/agent-doc/agent-doc-bugs2.md` to inspect the pane.
```

As I was typing this prompt, agent-doc CRDT appendent duplicate content from this document causing the document to be corrupted.

This prompt was also duplicated from a background agent-doc procedure.

I already prompted to fix these issues 2 times but they remain unfixed.
### Re: live typing duplicate and clear refusal — gpt-5

Fixed both failure modes and installed the patched build.

- `session clear <FILE>` no longer refuses solely because the actor projection is stale `starting` while the pane is alive/busy and the document has prompt-only drift. It now allows clear from direct pane evidence when there is no protected prompt input or clean-exit restart prompt, and logs `session_operator_starting_clear_allowed` for that boundary.
- IPC closeout now compares the before/after document and removes duplicate prompt lines introduced during live typing before the snapshot is committed. Repairs log `ipc_prompt_duplicate_repaired`, while the existing response-block duplicate guard remains in place.
- Updated `SPEC.md`, the split specs, and `VERSIONS.md` so the code-enforced behavior and documented invariants match.
- Committed the `src/agent-doc` code changes as `a9c67fc8 fix: repair live prompt duplicate closeouts`.

Verification completed:

- `cargo fmt`
- `cargo test starting_clear_guard_allows_unprotected_busy_direct_pane`
- `cargo test ipc_snapshot_dedupes_extra_prompt_copy_against_before_content`
- `cargo test exchange_write_diagnostic_logs_live_edit_provenance`
- `cargo test`
- `make check`
- `make install`
- `make tmux-ci` locally, including the ignored live tmux sweep (`228 passed`)

CI review: latest GitHub Actions run `25773430221` from 2026-05-13 was green for the regular `check` job. That run did not expose a separate tmux job, so I used the local `make tmux-ci` sweep as the tmux evidence for this turn.
<!-- agent:boundary:a727bf60 -->
<!-- /agent:exchange -->
###
<!--
-->

<!-- agent:queue -->
<!-- /agent:queue -->

## Pending / Not Built

<!-- agent:backlog -->
- [ ] [#34qd] [recommended] Add an end-to-end JetBrains Clear Session Context regression for a stale starting actor with live prompt-only document drift and duplicate prompt repair.
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
