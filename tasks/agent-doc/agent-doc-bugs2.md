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
queue: start
---

## Status

<!-- agent:status patch=replace -->

done
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7a9b103bd5b68223dac05a4c449ac03899aba53d6022e82100578b5b0-20260624-203916.md`*

Compacted content:
- Archived 19 response topic(s): do #ipcproofcloseout, #crdtreset1, and #routeblockux1; stalled Run Agent Doc recovery; dropped recovery evidence prompts; 16 more
- Prior summary/context: ❯ - JetBrains `TerminalUtilTest.protected prompt input route refusal is actionable not persistent failure` ❯ - `make ...

### Re: IPC integration and monsterrodholders queue audit — gpt-5

Short answer: not full if “using lazily-kt/lazily-js” means the plugins consume those standalone packages directly. The core path is in place: `agent-doc-orchestration` depends on `lazily 0.12.2`; Rust exposes `agent_doc_state_projection` / `agent_doc_record_state_event`; the state-backbone spec requires editor bridges to bind that ABI, canonicalize document hashes, report editor patch queued/ack/retry and route events, and render projection slices.

JetBrains and VS Code do that today: JetBrains `StateProjectionBridge`, `PatchWatcher`, and `TerminalUtil` record transport and route facts and log projection summaries; VS Code `native.ts` and `extension.ts` do the same. `lazily-kt` and `lazily-js` each also have reusable `StateProjectionClient` wrappers plus tests. The remaining gap is that the plugins currently duplicate bridge code locally instead of consuming those packages as their integration boundary, and we still need live/cross-editor proof that both editors reduce to the same `DocumentStateProjection` during real patch and route flows.

> **Queue prompt:** agent-doc monsterrodholders.md queue was removed without being processed

I ran `agent-doc queue recover-lost /home/brian/work/btakita/agent-loop/src/boost-client/tasks/monsterrodholders.md`. It found 20 git-history-only unaccounted heads, including the test-email CSV prompt shown here, but `restore_candidates=0`: no current snapshot, baseline, or patch sidecar carried an authoritative restore candidate. So the removal is visible in git history but not safely restorable by the current recovery command.

Added backlog items in priority order. Top backlog id: `#lzpkgwire`.

Verification: `agent-doc preflight`, `agent-doc plan`, source inspection of Rust/JB/VS Code/lazily wrappers, and `agent-doc queue recover-lost /home/brian/work/btakita/agent-loop/src/boost-client/tasks/monsterrodholders.md`. No code changed, so I did not run `make check`.
<!-- agent:boundary:61afd884 -->
<!-- /agent:exchange -->

## Queue

<!-- agent:queue priority preset="#spec-test-build-install-commit-push" priority go -->
- 🚧 do [#71wr]
---
agent-doc monsterrodholders.md queue was removed without being processed
```
• The queue helper is contradictory here: it sees the preset as id-backed, then says there is no matching id-backed
  head. I’m removing the answered queue row directly and keeping the queue component itself intact.

• Edited tasks/monsterrodholders.md (+0 -1)
    82  <!-- agent:queue preset="#spec-test-commit-push" priority go -->
    83 -- We should have the test email addresses in a CSV file. brian.takita+mrh-user-1@gmail.com, brian.takita+mrh-us
        er-2@gmail.com, brian.takita+mrh-user-3@gmail.com, rberkes+mrh-user-1@gmail.com, rberkes+mrh-user-2@gmail.com,
        rberkes+mrh-user-3@gmail.com.
    83  <!-- /agent:queue -->

───────────────────────────────────────────────────────────────────────────────────────────────────────────────────────

• The answered queue row is removed. I’m committing the document cleanup through agent-doc so its snapshot stays
  aligned, then I’ll rerun the invariant check.
```
---
<!-- /agent:queue -->
<!--
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
- [ ] [#lzpkgwire] Consolidate package-level lazily state-projection bridge: make JetBrains consume lazily-kt and VS Code consume lazily-js (or formally mark plugin-local copies as canonical), then add parity tests for document hashing, event JSON, pointer/free lifecycle, and projection summaries.
- [ ] [#lzliveproof] Add cross-editor live/state-projection proof for agent-doc IPC: drive JB socket/file IPC and VS Code file IPC through queued/ack/retry plus route started/proven/blocked events, then assert the Rust DocumentStateProjection and compact summaries match.
- [ ] [#mrhqrestore] Improve agent-doc queue recover-lost for git-history-only queue deletions: produce an operator-reviewed restoration patch or explicit non-restorable recommendation for foreign-owned prompts such as the monsterrodholders test-email CSV row.
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
