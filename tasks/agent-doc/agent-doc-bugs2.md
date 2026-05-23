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

Top backlog item: #edproof. Added #tsiftipc as the concrete tsift.md reproduction fixture.
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7a9b103bd5b68223dac05a4c449ac03899aba53d6022e82100578b5b0-20260523-141156.md`*

Compacted content:
- Archived 2 response topic(s): #next-steps to fix agent-doc bugs; full-document IPC next steps
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7...
<!-- agent:boundary:68e0d637 -->
<!-- /agent:exchange -->
###
<!--
-->

<!-- agent:queue auto -->
preset #spec-test-build-install-commit-push
- do [#edproof]
- do [#typedrepair]
- do [#repairtests]
- do [#tsiftipc]
- do [#narrowpatch]
- do [#repaircleanup]
<!-- /agent:queue -->

## Pending / Not Built

<!-- agent:backlog -->
- [ ] [#edproof] [recommended] Require fresh editor-buffer proof before response-fallback full-document redelivery, so repaired snapshots are not pushed over unrelated live edits.
- [ ] [#typedrepair] [recommended] Split write.rs IPC repair outcomes into typed decisions with repaired snapshot content, disk-repair reason, editor bad-state fingerprint, redeliver_editor, and structured ops-log records.
- [ ] [#repairtests] [recommended] Add deterministic unit and SimWorld coverage for ack-sidecar-only repair, real visible duplicate repair, sidecar normalization divergence, stale source-buffer skips, and post-commit user-follow-up drift.
- [ ] [#tsiftipc] [recommended] Capture the tsift.md duplicate-content corruption while typing as a named fixture and assert full-document IPC redelivery is skipped without fresh editor-buffer proof.
- [ ] [#narrowpatch] [recommended] Replace eligible prefix-normalization fallback fullContent redeliveries with narrow normalization/reposition patches before using fullContent as a last resort.
- [ ] [#repaircleanup] [recommended] Update editor specs/tests for snapshot-only repair versus editor-redelivery behavior and remove obsolete workaround branches after typed repair decisions land.
<!-- /agent:backlog -->

<!-- agent:icebox -->
<!-- /agent:icebox -->

## Completed / Reaped

<!-- agent:done archive=tasks/agent-doc-bugs2.done.md -->

<!-- completed work archived in tasks/agent-doc-bugs2.done.md -->
<!-- /agent:done -->
