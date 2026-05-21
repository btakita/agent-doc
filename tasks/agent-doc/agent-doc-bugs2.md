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

Added auto-DAG next steps; top backlog item: #autodag.
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7a9b103bd5b68223dac05a4c449ac03899aba53d6022e82100578b5b0-20260521-215448.md`*

Compacted content:
- Archived 2 response topic(s): duplicate prompt corruption; duplicate prompt comment shell
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7...
<!-- agent:boundary:f4046930 -->
<!-- /agent:exchange -->
###
<!--
-->

<!-- agent:queue -->
preset #spec-test-build-install-commit-push
- do [#autodag]
- do [#dagdispatch]
- do [#dagrecover]
- do [#daglogs]
<!-- /agent:queue -->

## Pending / Not Built

<!-- agent:backlog -->
- [ ] [#missedpb] Missed-patchback false-positive in finalize / write --commit cycle. REPRO: prior turn's finalize lands a commit but the response body (patch:exchange block) never materializes in the file. Subsequent agent-doc finalize reports 'rejecting late fallback patch: cycle <id> already committed for <file>' AND agent-doc write --commit reports 'staged snapshot already matches HEAD for <file> — closing cycle as already committed' + 'detected post-commit local drift', yet session-check correctly detects 'live exchange ends with unresolved prompt-only closeout tail and no assistant response'. Net: state machine thinks the cycle closed when the visible response was never inserted. WORKAROUND: direct Edit + git commit. EVIDENCE: tasks/root.md cycle on 2026-05-21 ~17:23 EDT, commit 521ab300 in session-share landed a finalize commit with NO response body in the file; required manual repair via commit 98ff9008 ('agent-doc(root): manual repair — inject missing response body'). FIX PLAN: (1) in write --stream, detect when IPC delivery completed but the patch:exchange materialization step was skipped (sidecar normalization fallback path) and surface a hard error instead of silently committing; (2) in commit/finalize, treat 'snapshot already matches HEAD' as success ONLY when the snapshot itself contains the expected response body — currently the check passes on equality without verifying the response is present; (3) when session-check INTERRUPTED is raised post-commit, automatically open a write --commit-repair cycle that re-delivers the response from the last captured cycle state instead of returning success. DO NOT IMPLEMENT THE FIX.
- [ ] [#autodag] [recommended] Add an agent-doc auto-DAG planner for queued do/compound directives: parse explicit depends-on text, backlog ids, graph/conflict evidence, and job packets into a topological schedule; fail closed on cycles, missing ids, stale graph evidence, or ambiguous ownership before dispatch.
- [ ] [#dagdispatch] [recommended] Execute auto-DAG batches through agent-doc orchestrate/jobs: run parallel-safe antichain batches, collect worker_result proof, unblock downstream nodes only after required proof/session-check passes, and stop deterministically on failed/gated nodes without launching dependents.
- [ ] [#dagrecover] [recommended] Add auto-DAG persistence/recovery semantics: write operation docs/job packets with schedule id, node state, attempt count, replay commands, and repair commands so interrupted DAG runs can resume idempotently after compact/finalize/restart.
- [ ] [#daglogs] [recommended] Use session-review guardrails as scheduler input: classify prompt-budget/cache-resend/restart-loop/noop-closeout log families before auto-DAG start and gate large or looping runs into compact/restart/fixture-fix steps.
<!-- /agent:backlog -->

<!-- agent:icebox -->
<!-- /agent:icebox -->

## Completed / Reaped

<!-- agent:done archive=tasks/agent-doc-bugs2.done.md -->

<!-- completed work archived in tasks/agent-doc-bugs2.done.md -->
<!-- /agent:done -->
