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

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7a9b103bd5b68223dac05a4c449ac03899aba53d6022e82100578b5b0-20260519-043705.md`*

Compacted content:
- Archived 2 response topic(s): log review / next steps; rtbr
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7...
❯ do [#scaf2]. #spec-test-build-install-commit-push
### Re: scaf2 and live CRDT write — gpt-5

`#scaf2` is completed and reaped. The guard is present in `src/agent-doc`: the normal template closeout path now tries `repair_duplicate_exchange_close_scaffold` before the older duplicate-close tail repair, repairs pure duplicated scaffold, and rejects mixed scaffold plus live user text.

Your follow-up question exposed the remaining mechanism: agent-doc does not need another agent turn to be running to write while you type. The `finalize`/`write --stream` closeout itself is a document writer. In this run it used socket IPC, the editor-side path accepted the patch while you were editing the scaffold HTML comment, and the visible file briefly became malformed before commit refused it because the snapshot size no longer matched the visible file. That is better than committing corruption, but it is still not good enough because the editor-visible document was mutated before the fail-closed boundary.

I repaired the live document without dropping your text, reset the sidecars from the repaired visible file, and used a force-disk committed write for this response to avoid re-entering the same IPC path. I also added a new backlog item for the remaining IPC-visible-write gap.

Verification completed:
- `cargo test scaffold --lib -- --test-threads=8`
- `cargo test --all-targets normalize_template_structure_ -- --test-threads=8`
- `make check TEST_THREADS=8`
- `make tmux-ci TMUX_TEST_THREADS=1` (228 tmux/ignored tests passed)
- `cargo build --release`
- `cargo install --path .`
- `cargo build --release --lib`
- `agent-doc lib-install`

CI check: latest `CI` workflow on `main` is green (`fix: repair main ci checks`, run `25773430221`, completed 2026-05-13). The scaf2 plan artifact is committed as `d317ed90a`; the session recovery/reap commit is `62d535134`.
<!-- agent:boundary:62418af8 -->
<!-- /agent:exchange -->
###
<!--
Re [#scaf2]: Why is there a duplicate CRDT operation happening in the first place? No turn is running so why does agent-doc attempt to corrupt the document? Note the corruption happened as I replaced content in the html comment under agent:exchange in this document? Why is content being written into the document as the user types?
spec-test-build-install-commit-push
-->

<!-- agent:queue -->
<!-- /agent:queue -->

## Pending / Not Built

<!-- agent:backlog -->
- [ ] [#nd87] [recommended] Fix the IPC visible-write stale snapshot path for concurrent scaffold/comment edits: finalize delivered a socket IPC patch while the user edited the scaffold HTML comment, produced a malformed exchange/scaffold rewrite in the visible file, then refused commit on stale snapshot size. Ensure IPC sidecar/ack paths run the duplicate-scaffold guard before mutating the editor-visible document or fail closed before editor write.
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
