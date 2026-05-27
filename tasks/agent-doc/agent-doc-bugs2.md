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
  '#mermaid': Include explanatory mermaid diagram(s).
  '#spec-test-build-install-commit-push': update spec + tests. build + install for local testing. commit + push
  '#spec-test-commit-push': update spec + tests. commit + push
queue_active: true
---

## Status

<!-- agent:status patch=replace -->

Live IntelliJ File Cache Conflict Cancel proof captured; JB Cancel residual review items completed.
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7a9b103bd5b68223dac05a4c449ac03899aba53d6022e82100578b5b0-20260527-210358.md`*

Compacted content:
- Archived 3 response topic(s): JetBrains Run Agent Doc queued reroute; JB Run Agent Doc on tsift.md queue stall; #spec-test-build-install-commit-push
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7...
JB `Run Agent Doc` on this document did not run agent-doc in the Codex console.
#spec-test-build-install-commit-push #mermaid

### Re: #spec-test-build-install-commit-push — gpt-5

Implemented and pushed the second queue-closeout fix. I also picked up the new `#mermaid` preset definition and included the requested diagram below.

The original JB `Run Agent Doc` failure had two separate closeout breaks:

1. `route --dispatch-only` could save a busy-actor reroute into `agent:queue auto` and snapshot it, while `HEAD` still lacked that queued handoff. A later live prompt edit made preflight see `snapshot != HEAD` and `working tree != snapshot`, so it stopped before it could drain the queue.
2. After that was fixed, this live queued cycle exposed a second bug: a synthetic queue prompt has no document diff between the baseline and current file, so `finalize` wrote the response but skipped queue consumption. The new fix lets `finalize` re-check the captured response after writing and consume the active queue head when the `### Re:` topic explicitly targets the queue head id.

```mermaid
flowchart TD
  A["JB Run Agent Doc"] --> B["Actor busy"]
  B --> C["route saves prompt in agent:queue auto"]
  C --> D["Snapshot includes queued prompt"]
  D --> E["User edits document before next preflight"]
  E --> F["Old preflight sees snapshot and HEAD drift"]
  F --> G["Queue prompt is not sent"]
  G --> H["Fix 1: preflight commits route-owned queued snapshot first"]
  H --> I["Queued prompt becomes active synthetic prompt"]
  I --> J["Old finalize writes response but sees no user diff"]
  J --> K["Queue head remains active"]
  K --> L["Fix 2: finalize checks captured response heading"]
  L --> M["Heading targets queue id"]
  M --> N["Queue head consumed"]
```

Changed paths:
- `src/agent-doc/src/preflight.rs`
- `src/agent-doc/src/write.rs`
- `src/agent-doc/tests/finalize_integration.rs`
- `src/agent-doc/SPEC.md`
- `src/agent-doc/specs/07-closeout-commands.md`
- `src/agent-doc/VERSIONS.md`

Commits pushed:
- `btakita/agent-doc` branch `20260422/codex-agent-backend`: `62cdbbe1` and `ab8ac7a2`
- `btakita/agent-loop` `main`: `18786a6e7` and `bcd6394af`

Verification:
- `cargo test preflight_commits_route_queue_snapshot_before_live_prompt_edit` passed.
- `cargo test finalize_consumes_synthetic_queue_prompt_when_response_topic_targets_head_id` passed.
- `make check` passed locally through `tsift` artifact `tart-d5dcb48ae3`.
- `make install` passed locally through `tsift` artifact `tart-e39bc65ae4`.
- GitHub Actions CI run `26537671598` passed, including both `make check` and `make tmux-ci`. The only annotation was the existing Node.js 20 deprecation notice for `actions/checkout@v4`.

Escalation: none. Confidence: high; this cycle reproduced the remaining queue-consumption failure live and the installed binary is now using the fix.
<!-- agent:boundary:2881a0f1 -->
<!-- /agent:exchange -->
###
<!--
The completed queue item was not removed from the queue. #mermaid
#spec-test-build-install-commit-push
---
equityfundingsource.md had the full-document IPC duplicate corruption. Then a new full-document IPC deletion under the prompt corruption happened. Both several times while editing the prompt.
#spec-test-build-install-commit-push
---
Please review the agent-doc, claude, codex logs. What are #next-steps to fix bugs?
---
When I ran the last turn, #spec-test-build-install-commit-push was removed by agent-doc.
---
As I was typing into the comment below `/agent:exchange`, the full-document IPC corruption happened, then the duplicate line happened, then the full-document IPC corruption happened again.

In addition to diagnosing and fixing the issue, please provide a mermaid showing how the issue occurred and where in the agent-doc lifecycle the full-document IPC corruption can occur. Can we mathematically eliminate all possibility of the full-document IPC corruption from occurring?

#spec-test-build-install-commit-push
-->

<!-- agent:queue auto -->
preset #spec-test-build-install-commit-push #mermaid
- do [#odsp2]
- do [#ocdsp]
---
JB `Run Agent Doc` on this document did not run agent-doc in the Codex console.
#spec-test-build-install-commit-push #mermaid
---
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog -->
- [ ] [#odsp2] Implement OpenCode dispatch-start proof via pane state transition detection — plan at tasks/agent-doc/plan-opencode-dispatch-start-proof.md
- [ ] [#ocdsp] Implement OpenCode dispatch-start proof via pane state transition detection — plan at tasks/agent-doc/plan-opencode-dispatch-start-proof.md — extend RoutedDispatchStartTracker, add PaneStateChanged proof variant, poll capture-pane for idle→active transition
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
