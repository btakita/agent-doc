---
agent_doc_session: b26b9957-8de6-4eae-bb98-5d7fe4d6b781
agent: opencode
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

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7a9b103bd5b68223dac05a4c449ac03899aba53d6022e82100578b5b0-20260624-213413.md`*

Compacted content:
- Archived 7 response topic(s): IPC integration and monsterrodholders queue audit; IPC proof issue dogfood log; queue repair; 4 more
- Prior summary/context: *Compacted earlier. Prior content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7a9b103...

### Re: JB Run Agent Doc on equityfundingsource.md — codex→opencode switch stranded — glm-5-2

Root cause: the error is **not a regression** — it's the `#agentreloadrestart` kill-guard doing exactly what plan-restart-agent.md added after this same doc's earlier mid-turn kill. `route/authoritative_actor.rs:40-95` sees the stored actor harness (`codex`) differ from the frontmatter harness (`opencode`) with `frontmatter_harness_changed=true`. `mismatched_authoritative_actor_can_be_replaced` returns `false` because the old codex actor is healthy/non-closed (test `..._only_when_not_live_authority`), so route logs `route_authoritative_actor_harness_mismatch_deferred action=defer_to_boundary_restart` and bails instead of cold-replacing the live pane.

The deferred work is owned by the **supervisor idle-watch** (`start/idle_watch.rs:715-803`): each tick it re-resolves `agent:` from frontmatter, and at a `prompt_visible && !turn_active` boundary `agent_change_restart_decision` returns `Restart` → requests a fresh restart → `run.rs` respawns opencode (`agent_restart_performed`). Phase 1a detection + Phase 1b execution are both shipped.

**The actual bug: the defer is a dead-end.** It dead-ended here because the supervisor never reached the boundary — and the bail gave you no way to know that or force it:
- this session is `queue_paused: true` (stale `#rt83/#qflood` pause), so the idle-watch wasn't ticking → `harness_change_detected` never fired;
- or the codex pane never returned to a dispatch-ready prompt (`turn_active` / not `prompt_visible`) → decision stuck at `WaitForBoundary`;
- or `agent_change_restart_enabled` resolved off.

In every branch, route's "deferring to boundary agent restart" is the last thing you see.

**Next step (captured):** added `#actorswitchdefer` (top of backlog) + plan `tasks/agent-doc/plan-actor-switch-defer-recovery.md`. Two parts — (A) enrich the route bail with supervisor health/pause state and a concrete `agent-doc session restart-supervisor <FILE>` recovery hint (small, safe, unblocks today); (B) SimWorld proof that the idle-watch actually fires the restart when deferred (or surfaces paused/stuck), plus reconciling SPEC.md:141 (which still says "fall back to create/rebind," contradicting the shipped defer). The fail-closed rule for healthy actors stays — this only fixes the message and proves the deferred path.

Note: the queue on this doc is still `queue_paused` (stale supervisor pause from `#rt83/#qflood`; we're now on 0.34.43, well past the "install 0.34.20 + recycle supervisor" resume condition). `#mrhqrestore` and `#ipcqproof` remain gated. Say the word and I'll (1) run Part A of `#actorswitchdefer` in a focused cycle, and/or (2) recycle the supervisor so `#mrhqrestore`/`#ipcqproof` can drain.
<!-- agent:boundary:da548d3d -->
<!-- /agent:exchange -->

## Queue

<!-- agent:queue priority preset="#spec-test-build-install-commit-push" priority -->
- 🚧 do [#mrhqrestore]
- do [#ipcqproof]
- Still getting JB `File Cache Conflict` dialogs.
- Opencode responses are reverse ordering numeric lists.
---
JB `Run Agent Doc` on equityfundingsource.md after switching from codex to op
```
resync: pruned 2 stale session(s)
[route] target tmux session: 0
Error: authoritative actor record for tasks/professional/equityfundingsource.md is running harness codex, but frontmatter now resolves to opencode; deferring to boundary agent restart instead of replacing live pane
```
---
- 🚧 do [#mrhqrestore]
- do [#ipcqproof]
- do [#actorswitchdefer]
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
- [ ] [#actorswitchdefer] Route defer on harness switch (codex→opencode) dead-ends with no operator recovery: enrich the `defer_to_boundary_restart` bail in route/authoritative_actor.rs with supervisor state + a `session restart-supervisor <FILE>` hint, and add SimWorld proof the idle-watch fires the restart (or surfaces paused/stuck). Plan: tasks/agent-doc/plan-actor-switch-defer-recovery.md
- [ ] [#mrhqrestore] Improve agent-doc queue recover-lost for git-history-only queue deletions: produce an operator-reviewed restoration patch or explicit non-restorable recommendation for foreign-owned prompts such as the monsterrodholders test-email CSV row.
- [ ] [#ipcqproof] Fix IPC proof dogfood closeout recovery: add a regression for queue-consume socket_ack_content / live_prompt_drift_after_preflight ACK mismatch, ensure diagnostics do not become unresolved prompt-bearing exchange items, and keep fail-closed recovery on the binary-owned path.
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
