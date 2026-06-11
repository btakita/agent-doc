---
agent_doc_session: b26b9957-8de6-4eae-bb98-5d7fe4d6b781
agent: claude
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
  '#mermaid': Include explanatory mermaid diagram(s).
  '#next-steps': Any follow-up items to place in the backlog? Please ensure the backlog is in priority order. Print the item id of the top of the backlog.
  '#spec-test-build-install-commit-push': update spec + tests. build + install for local testing. commit + push
  '#spec-test-commit-push': update spec + tests. commit + push
queue: start
---

## Status

 <!-- agent:status patch=replace -->

completed
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7a9b103bd5b68223dac05a4c449ac03899aba53d6022e82100578b5b0-20260611-184404.md`*

Compacted content:
- Archived 9 response topic(s): #next-steps to fix bugs before release; JB `Run Agent Doc` on Codex still doesn't submit the prompt; JB `Run Agent Doc` on Codex submit fix; 6 more
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7...

### Re: queue flood — root cause refined; binary IS now in-process — opus-4-8

Cleared the struck "unrelated queue item that stalled the queue" — that was bookkeeping, no action needed.

The substantive part: I pulled real `ops.log` ground truth this turn and it **corrects the prior cycles' conclusion**.

**1. The new binary is running.** `ops.log` shows `supervisor_host_gate ... hosting=in-process` (generation 300), `controller_supervisor_registered`, normal lifecycle transitions. The 08b in-process cutover is **live in this session** — the earlier "live controllers run the pre-`e6f7538e` binary, nothing is running" diagnosis is stale. Every fix shipped this session is actually executing now.

**2. The flood comes from a different code path than `#kp5z`'s observability targets.** There are **zero** real `queue_dispatch_invoked` markers — not because the binary is old, but because that marker lives in `project_controller::authorize_dispatch`, which is **not** the path that injects the trigger. The real injector is the **supervisor** `idle_queue_watch_drain` in `start.rs`: `payload_kind=trigger submit_mode=tmux_literal_cr target=%123 reason=auto_trigger_inject`. The `7b6f022e` observability was added to the wrong path.

**3. That real path is already gated.** `idle_queue_drain_decision` already has the busy gate (`SkipNotIdle` / no-inject-into-active-turn), the dedup (`SkipAlreadyDispatched`), and real observability (`log_idle_queue_drain_submit`). So the residual flood isn't a missing gate — it's a **second drain owner**: Claude Code `/loop` and the supervisor watch **both** inject `agent-doc <FILE>`, so triggers stack up in the live CC input queue (exactly what you keep having to delete by hand). Fix target: a self-driving-harness gate in `idle_queue_drain_decision` that skips the supervisor inject when `/loop` is the active drain owner — pure logic, unit-testable. I refined `#kp5z` with this.

**What I did this turn:** paused the queue (`receipt_id=7`, observed-generation 300) so the supervisor stops re-injecting triggers into your live pane while this is fixed. **Not auto-looping** — there's no new prompt (only the struck item), the queue floods on drain, and the actual fix needs a dedicated stable turn with live multi-process verification that **cannot** happen from inside this affected session (the supervisor races my writes here).

`#nzmc` still network-blocked (crates.io offline). `#sjwm`/`#kqr6` still need that same clean turn. The review items remain operator-drive live proofs.
<!-- agent:boundary:55b29ada -->
<!-- /agent:exchange -->

## Queue

<!-- agent:queue priority preset="#spec-test-build-install-commit-push" go -->
- test test
<!-- /agent:queue -->
###
<!--
Please make backlog replacements for all gated review items. We need to complete the plan, not have any more gates after this round. Let me know when I need to install JB plugin updates. Make sure all JB live gate items have adequate logging to prove/disprove the item. Clear the review items. I will test, files bugs, and you will check the logs.
---
`'/clear' is disabled while a task is in progress
---
A recent monsterrodholders.md session had much churn after I edited the queue while a turn was running. One churn item included deleting a queue item that I added.
---
agent-doc reentrant run on equityfundingsource.md had recovery churn.
```
● The recovery is complete and fully consistent. The binary converged the now-drained queue (empty block, queue_active:
  false), no_changes: true, both responses are committed in the session doc, and both code deliverables are on
  origin/main. The auto-loop terminated naturally.

  Let me record the specific recovery path for this failure mode, since it's a recurring dogfooding pattern
```
---
- This session's console stdout is bleeding in front of the opencode tui. ![img_16.png](img_16.png)
---
The last run had a code fence stripped away by agent-doc.
#spec-test-build-install-commit-push
---
What are #next-steps to fix bugs?
-->

## Backlog

<!-- agent:backlog priority queue --> 
- [ ] [#sjwm] #ipcdrift IPC drift continues post single-Project-Pane-Process / 08b migration. CONFIRMED root cause overlaps #stuckhandoff: controllers wedge in ControllerHandoffState::Preparing and keep writing the stale buffer (multi-owner race on ipc.sock vs the IDEA listener). The handoff state machine stores handoff_started_at but has NO stale-Preparing timeout/reaper (verified in project_controller.rs). Fix = stale-Preparing detect + force-fail/supersede + terminate the wedged live controller + self-heal on invoke. NEEDS a dedicated stable turn + live multi-process verification (cannot be verified from inside the affected session). Plan: tasks/agent-doc/plan-queue-dispatch-flood.md (IPC section TBD).
- [ ] [#kqr6] #stuckhandoff CONFIRMED: no handoff-timeout scaffold exists (project_controller.rs ControllerHandoffState has Preparing + handoff_started_at but nothing checks staleness). gc.rs reaps stale starting actor PROJECTIONS (>1h) but not wedged Preparing controller PROCESSES, and a projection-close would not stop a live wedged proc from writing. Real fix = detect stale Preparing by handoff_started_at age + terminate/transition the wedged controller + heal on next invoke, with live verification. IMMEDIATE RELIEF (operator-run): pkill the wedged controller serve --handoff-state preparing PIDs; they respawn clean. Operator request 2026-06-11.
- [ ] [#nzmc] #lazilyup Upgrade lazily-rs (tsift is upgrading; build already resolves lazily 0.10.3 in Cargo.lock). Verify build + commit the lock bump. Operator request 2026-06-11.
- [ ] [#kp5z] #qflood [TOP] Queue dispatch floods the running harness session with repeated agent-doc trigger submissions during queue-drain / finalize-while-typing. NEW FINDING (this turn, fresh-binary session): the new in-process binary IS now running (ops.log: supervisor_host_gate hosting=in-process, generation 300) — prior cycles blamed a stale old-binary controller; that is no longer the blocker. The actual trigger-injection path is the SUPERVISOR idle_queue_watch_drain in start.rs (ops.log: idle_queue_watch_drain payload_kind=trigger submit_mode=tmux_literal_cr target=%123 reason=auto_trigger_inject), NOT project_controller::authorize_dispatch — that is why zero real queue_dispatch_invoked markers ever logged: the 7b6f022e observability was added to the wrong path. The real path ALREADY has the busy gate (idle_queue_drain_decision::SkipNotIdle / no-inject-into-active-turn), the dedup (SkipAlreadyDispatched), and real observability (log_idle_queue_drain_submit). REFINED ROOT-CAUSE HYPOTHESIS: dual drain owners — Claude /loop AND the supervisor idle_queue_watch_drain both inject agent-doc <FILE>, so triggers pile up in the live CC input queue. FIX TARGET: a self-driving-harness gate in idle_queue_drain_decision (skip supervisor inject when /loop is the active drain owner) — pure logic, unit-testable in start.rs. NEEDS a dedicated stable turn + live multi-process verify (cannot be confirmed from inside this affected session; supervisor races writes here). Plan: tasks/agent-doc/plan-queue-dispatch-flood.md.
<!-- /agent:backlog -->

## Review
 
<!-- agent:review -->
- [/] [#w42v] #jbcompactconflict JB Compact Exchange writes the full doc to disk (atomic_write_if_current_pub, compact_exchange_direct_write), diverging from the open JB buffer = File Cache Conflict. The full-content editor-IPC path for compact (try_ipc_full_content_operator_mutation, source label compact_exchange) EXISTS but is HARD-DISABLED by design (always returns Ok(false), reason=disabled_by_default). Making it CRDT-driven = re-implement+re-enable that path with a component-scoped exchange op:replace convergence (mirror #q7jm) + JB-side apply + live verify. NEEDS OPERATOR DIRECTION before implementation (it was disabled deliberately). Plan: tasks/agent-doc/plan-jb-compact-crdt.md.
- [/] [#qt32] #lvbremain Operator live zero-drift PROOF of the 08b supervisor-in-process cutover (cannot be agent-done — needs a live editor). Restart a session, drive one edit-during-finalize cycle, agent reads ops.log for write_authority action=routed + plugin_watch_readonly + supervisor_host_gate hosting=in-process + ZERO drift lines. [recommended]
- [/] [#2v7b] #jbcodexsubmit JB `Run Agent Doc` on Codex enters the routed prompt but does not submit it (operator must press Enter manually). Root cause: Codex direct-pane dispatch in route.rs::send_command_unchecked detects prompt_not_submitted (TriggerStillVisible) but has no re-submit fallback; the merged text+`\r` send-keys payload is not treated as submit by the Codex TUI. Fix: Codex-scoped one-shot Enter re-submit + `route_submit_resubmit` proof logging + SimWorld coverage. Plan: tasks/agent-doc/plan-jb-codex-submit.md. Live verify is operator-drive (check ops.log for route_submit_resubmit action=enter_key result=accepted).
<!-- /agent:review -->

<!-- agent:icebox -->
<!-- /agent:icebox -->

## Completed / Reaped

<!-- agent:done archive=tasks/agent-doc-bugs2.done.md -->

<!-- completed work archived in tasks/agent-doc-bugs2.done.md -->
<!-- /agent:done -->
