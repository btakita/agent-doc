---
agent_doc_session: b26b9957-8de6-4eae-bb98-5d7fe4d6b781
agent: codex
claude_model: opus
agent_doc_format: template
agent_doc_write: crdt
claude_args: --dangerously-skip-permissions
codex_args: -s danger-full-access
pending_done_guard: warn
agent_doc_lint_dialect: off
prompt_presets:
  '#advance-review': Go through the review items. Add actionable backlog + queue items to complete or advance the review items.,
  '#agent-doc-bug': Please create a plan for agent-doc to fix this issue. Add to the backlog of tasks/agent-doc/agent-doc-bugs2.md
  '#code-review': Please review agent-doc codebase. Inspect the architecture and look for edge cases. Isolate concurrency issues and other workflow bugs. Let's find root causes to fix.
  '#commit-push': commit + push all uncommitted files
  '#follow-up-backlog': Any follow-up items to place in the backlog?
  '#mermaid': Include explanatory mermaid diagram(s).
  '#next-steps': Any follow-up items to place in the backlog? Please ensure the backlog is in priority order. Print the item id of the top of the backlog.
  '#simworld': Strengthen Simworld tests.
  '#spec-test-build-install-commit-push': update spec + tests. build + install for local testing. commit + push
  '#spec-test-commit-push': update spec + tests. commit + push
queue: stop
---

## Status

<!-- agent:status patch=replace -->

done
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

❯ *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7a9b103bd5b68223dac05a4c449ac03899aba53d6022e82100578b5b0-20260624-073352.md`*

Compacted content:
❯ - Archived 11 response topic(s): JetBrains markdown emphasis colors; JetBrains markdown emphasis colors; JetBrains cold-open markdown highlighting - gpt-5; 8 more
❯ - Prior summary/context: ❯ - submodule: `a788e71e3` (`fix: keep component body text on theme foreground`), pushed to `agent-doc/main` ❯ - push...
❯ **IPC proof issue dogfood log**

❯ Detected during interrupted-cycle recovery. This note is appended automatically so the session document records editor IPC issues while queue edits are being dogfooded.

❯ Issue class: `ipc_proof_insufficient`
❯ Affected component: editor IPC / writeback

```text
ipc_proof_insufficient file=/home/brian/work/btakita/agent-loop/tasks/agent-doc/agent-doc-bugs2.md source=socket_ack_content patch_id=e91d9537-8972-4d66-b358-497bd4a0c3b5 invariant=live_prompt_drift_after_preflight recovery=content_ours_snapshot_next_cycle snap_source=ack_content_sidecar candidate_len=25209 candidate_hash=517cc9774065e4e6053723e7d7022c55759e0a2afcc5b69c364f4e014ca3c9b1 content_ours_len=25557 content_ours_hash=4f3e645e0e8cd2b2e00ab5623371e8a2d59090e100b3e728534d11f35da8cc3e doc=agent-doc-bugs2 session=b26b9957-8de6-4eae-bb98-5d7fe4d6b781 turn=cycle-1782286092889
```

### Re: IPC proof issue dogfood log — gpt-5

> **Queue prompt:**
>
> do [#docdriftfinalize]


The IPC proof note is acknowledged. This cycle hit the stale snapshot/CRDT sidecar guard after compaction: finalize refused to write because the snapshot was still much larger than the visible compacted file, so I ran `agent-doc reset --from-current /home/brian/work/btakita/agent-loop/tasks/agent-doc/agent-doc-bugs2.md` and reran preflight. The recovery rebuilt sidecars from the visible document, reaped the stale resolved `#4v2w` queue head, cleared the queue, and set `queue: stop`.

I added follow-up backlog item `#docdriftfinalize` to extend the existing `#docdriftgrace` plan to this finalize/write refusal path. The current top backlog id is `#docdriftfinalize`.

Proof:
- Commands: `tsift status`, `tsift status --fix`, `agent-doc preflight`, `agent-doc plan`, `agent-doc read`, `agent-doc session-check`, `agent-doc reset --from-current`, `rg` searches for the related drift/backlog history, and `git status --short`.
- Changed paths: this session document and its done archive through agent-doc recovery/finalize metadata; no product code changed in this cycle.
- Verification: recovery preflight completed, queue continuation is no longer required, and finalize owns the post-commit `session-check` boundary.
- Confidence: high.
- Escalation: none.
- Root `agent-loop` has no workflow named `CI`; its `Agent Doc` run `28083396280` failed before any runner steps started. Both jobs have the GitHub annotation: recent account payments failed or the spending limit needs to be increased, so there are no failed test logs to fix.
- `agent-doc` CI run `28083363360` for `d5ea2949` was still `in_progress` when I closed this item.
- Root commit `219ea93b6` (`agent-doc: accept compact summary snapshot rebase`) pushed to `btakita/agent-loop main`.
- `src/agent-doc` commit `d5ea2949` (`fix: accept compact summary snapshot rebase`) pushed to `btakita/agent-doc main`.
- `agent-doc admin recycle /home/brian/work/btakita/agent-loop/tasks/agent-doc/agent-doc-bugs2.md` marked the controller for recycle at the next idle boundary.
- Root `make check` passed after the superproject pointer update.
- `make precommit` passed in `src/agent-doc`.
- `make install` installed the updated local binary/library and marked controllers for recycle.
- `make check` passed in `src/agent-doc`.
- `cargo test -p agent-doc --test test_cli flowcore_hot_path_guard_and_proof_tokens_are_budgeted -- --nocapture` passed.
- `cargo test -p agent-doc-orchestration stale_snapshot_reset_drift_ -- --nocapture` passed.
- Added stream-write positive/negative regressions and updated the FlowCore token budget, `SPEC.md`, and `VERSIONS.md`.
- Arbitrary `### Session Summary` rewrites without compact markers still fail closed.
- `guard_no_stale_snapshot_reset_drift` now treats compact-generated `### Session Summary` exchange replacements with compact archive markers as safe historical exchange reductions, so `finalize --stream` can refresh stale pre-compact snapshot/CRDT sidecars instead of refusing and forcing `agent-doc reset --from-current`.
<!-- agent:boundary:9982411a -->
<!-- /agent:exchange -->

## Queue

<!-- agent:queue priority preset="#spec-test-build-install-commit-push" priority go -->
---
JB `Run Agent Doc` blocked
```
[route] target tmux session: 0
Error: route refusing to dispatch agent-doc /home/brian/work/btakita/agent-loop/src/boost-client/tasks/monsterrodholders.md into pane %2 for /home/brian/work/btakita/agent-loop/src/boost-client/tasks/monsterrodholders.md because the composer contains protected prompt input (drafted prompt input); clear or submit that draft, then rerun agent-doc route snapshot_path=/home/brian/work/btakita/agent-loop/src/boost-client/.agent-doc/logs/route-submit/1782287179464-direct_pane_protected_prompt_input-codex-_2-af2e4b2fc107.txt
```
<!-- /agent:queue -->
<!--
- 🧪 test edit for [#qnodemerge4wire]
---
Please make backlog replacements for all gated review items. We need to complete the plan, not have any more gates after this round. Let me know when I need to install JB plugin updates. Make sure all JB live gate items have adequate logging to prove/disprove the item. Clear the review items. I will test, files bugs, and you will check the logs.
---
`'/clear' is disabled while a task is in progress
---~~~~
A recent monsterrodholders.md session had much churn after I edited the queue while a turn was running. One churn item included deleting a queue item that I added
---
agent-doc reentrant run on equityfundingsource.md had recovery churn.
```
● The recovery is complete and fully consistent. The binary converged the now-drained queue (empty block, queue_active:
  false), no_changes: true, both responses are committed in the session doc, and both code deliverables are on
  origin/main. The auto-loop terminated naturally.

  Let me record the specific recovery path for this failure mode, since it's a recurring dogfooding pattern
```
---
The last run had a code fence stripped away by agent-doc.
#spec-test-build-install-commit-push
---
What are #next-steps to fix bugs?
-->

## Backlog

<!-- agent:backlog priority queue -->
<!-- /agent:backlog -->

## Review
 
<!-- agent:review -->
<!-- /agent:review -->

<!-- agent:icebox -->
- [ ] [#qloop1] #agent-doc-feature Define durable queue loop annotation syntax and parser/rendering behavior: mark loop-only queue/backlog entries, keep annotation/state out of prompt completion proof, and expose a stable current-item marker for editor/supervisor surfaces.
- [ ] [#qloop2] #agent-doc-feature Implement idle-loop scheduling for annotated loop items: when normal drainable queue work is empty, dispatch loop items in deterministic sequence, persist the loop cursor/last-run state, and prove regular queue items always preempt loop work.
- [ ] [#qloop3] #agent-doc-feature Add loop poll controls: global default poll period plus per-loop override, next-run time gates, bounded backoff/jitter, and tests proving no tight spin when there are no relevant events.
- [ ] [#qloop4] #agent-doc-feature Add event-aware loop scheduling: record deterministic event counters/timestamps and let loop items schedule earlier/later based on event frequency before any model-assisted scheduling hook.
- [ ] [#qobs1] #agent-doc-feature Add a bounded observer telemetry snapshot surface: supervisor/session actors expose compact per-document state (cycle phase, queue head, actor readiness, IPC inflight, recent typed ops-log markers, and artifact handles) without dumping raw transcripts by default.
- [ ] [#qobs2] #agent-doc-feature Implement deterministic observer rules that scan session-check/preflight/ops-log snapshots for annotated issues and recurring failure patterns, then file deduped session-doc backlog bugs with evidence paths and stable issue fingerprints.
- [ ] [#qobs3] #agent-doc-feature Add a sidecar observer agent-doc loop that can monitor all running project sessions from snapshot artifacts, rate-limit bug filing, avoid pane interference, and route created bug items to the correct session document or a configured observer inbox.
- [ ] [#qobs4] #agent-doc-feature Add model-assisted observer triage as advisory-only: feed compact snapshot/context-pack handles to a subscription or local model, require deterministic evidence before auto-filing bugs, and record model provenance/confidence on suggestions.
- [ ] [#qlm1] #agent-doc-feature Define queue item backend-selection syntax for local-model execution, separate from queued `/model <tier>` switches: support explicit per-item backend/model annotations on normal and loop queue items without changing prompt text or completion proof.
- [ ] [#qlm2] #agent-doc-feature Implement a local-model queue dispatcher backend that can run eligible queue/loop items through a configured local model, capture stdout/structured results, and write responses through the normal agent-doc closeout boundary.
- [ ] [#qlm3] #agent-doc-feature Add local-model dispatch safety gates and telemetry: capability checks, GPU/resource lease integration, fallback policy, provenance logs, and tests proving local-model queue items cannot be silently marked complete without a committed response/proof.
<!-- /agent:icebox -->

## Completed / Reaped

<!-- agent:done archive=tasks/agent-doc-bugs2.done.md -->

<!-- completed work archived in tasks/agent-doc-bugs2.done.md -->
<!-- /agent:done -->
