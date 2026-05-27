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
  '#next-steps': Any follow-up items to place in the acklog? Please ensure the backlog is in priority order. Print the item id of the top of the backlog.
  '#spec-test-build-install-commit-push': update spec + tests. build + install for local testing. commit + push
  '#spec-test-commit-push': update spec + tests. commit + push
queue_active: false
---

## Status

<!-- agent:status patch=replace -->

Live IntelliJ File Cache Conflict Cancel proof captured; JB Cancel residual review items completed.
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7a9b103bd5b68223dac05a4c449ac03899aba53d6022e82100578b5b0-20260527-074315.md`*

Compacted content:
- Archived 4 response topic(s): #snrun; closeout repair; equityfundingsource IPC corruption; 1 more
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7...
### Re: closeout repair — gpt-5

Closeout repair recorded the backlog-only `#next-steps` update. The priority-ordered backlog is present, and the top backlog item remains `#ipcmodel`.

The `#ipcmodel`, `#ipcreplay`, `#ipcdeny`, and `#ipcfail` entries were already captured as open backlog items in the prior commit. They are not executed tasks for this turn. I removed the accidental inactive-queue residue that made those ids look like executable queue directives. No repo code changed in this repair.

<!-- agent:boundary:00bb69bd -->
<!-- /agent:exchange -->
###
<!--
Please review the agent-doc, claude, codex logs. What are #next-steps to fix bugs?
---
When I ran the last turn, #spec-test-build-install-commit-push was removed by agent-doc.
---
As I was typing into the comment below `/agent:exchange`, the full-document IPC corruption happened, then the duplicate line happened, then the full-document IPC corruption happened again.

In addition to diagnosing and fixing the issue, please provide a mermaid showing how the issue occurred and where in the agent-doc lifecycle the full-document IPC corruption can occur. Can we mathematically eliminate all possibility of the full-document IPC corruption from occurring?

#spec-test-build-install-commit-push
-->

<!-- agent:queue -->
preset #spec-test-build-install-commit-push
- ~do [#snrun]~
- do [#jbrunbusy]
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog -->
- [ ] [#ipcmodel] Formalize and test an IPC closeout state model: enumerate allowed snapshot sources and state transitions for socket/file/direct-disk paths, then add property/table tests proving bad ACK content, prompt drift, partial response materialization, stale patch replay, and post-block snapshot absorb cannot reach commit.
- [ ] [#ipcreplay] Add a deterministic equityfundingsource IPC race replay harness that replays typing during finalize against socket IPC, file IPC, ACK sidecars, stale patch files, and direct-disk fallback so historical corruption packets stay covered outside live editor runs.
- [ ] [#ipcdeny] Add repo-wide receiver/static guards for full-document mutation APIs across CLI, JetBrains, and VS Code, with allowlisted parser/diagnostic references only, so legacy or foreign `fullContent` payloads remain unrepresentable as visible writes.
- [ ] [#ipcfail] Strengthen closeout diagnostics for insufficient IPC proof: surface the exact failed invariant and recovery path in preflight/session-check, and add tests for no ACK, missing response probe, duplicate prompt ACK, and live prompt drift.
- [ ] [#jbrunbusy] Fix JB Run Agent Doc active-turn retry classification: dispatch-only `latest run still booting ... active codex turn` refusals should be typed as busy/running, coalesced or retryable in the JetBrains plugin, and covered by route + plugin regression tests using the equityfundingsource repro.
<!-- /agent:backlog -->

## Review

<!-- agent:review -->
- [/] [#snrun] [recommended] Implement tasks/agent-doc/plan-run-agent-doc-snappy-auto-remediation.md: make JB Run Agent Doc distinguish rescued-stash accepted-only proof misses, active-turn busy collisions, and stale starting projections; add route/plugin regressions plus live replay evidence.
<!-- /agent:review -->

<!-- agent:icebox -->
<!-- /agent:icebox -->

## Completed / Reaped

<!-- agent:done archive=tasks/agent-doc-bugs2.done.md -->

<!-- completed work archived in tasks/agent-doc-bugs2.done.md -->
<!-- /agent:done -->
