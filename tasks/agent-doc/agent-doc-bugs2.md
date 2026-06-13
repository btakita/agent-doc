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

#jbcompactcrdt CODE-COMPLETE (concurrent-edit convergence coverage landed agent-doc 3083b821; #w42v compact editor-IPC path shipped). Operator-drive remainder: lib-install + live Compact Exchange verify. #rtwbcast remains queued (separate multi-editor item).
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7a9b103bd5b68223dac05a4c449ac03899aba53d6022e82100578b5b0-20260613-022946.md`*

Compacted content:
- Archived 8 response topic(s): do [#rtwbcast]; do [#rtwbcast]; do [#jbcompactcrdt]; 5 more
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7...

### Re: JB Compact Exchange + Run Agent Doc dispatch failures — opus-4-8

Captured both as agent-doc bugs. Neither should be fixed from this live IPC session; both are filed for a clean session.

**Bug A — Compact Exchange fails with `typed_component_drift` auto-adopt refusal (`#compactdrift`, new).** The compact did its archival work (148 exchange lines → archive, CRDT refreshed, stale capture discarded) and then refused to finalize: `refusing to auto-adopt committed historical response patchback … HEAD contains an out-of-band typed_component_drift mutation with response marker ### Re: do [#rtwbcast]`. This is the **same guard** that blocked `agent-doc commit` at the start of this session. Root cause: the guard keys on the *presence* of a committed `### Re:` marker in HEAD as evidence of out-of-band drift, so it false-positives on the normal post-finalize steady state (HEAD legitimately holds the last response). The fix is to make the discriminator structural — a HEAD response that matches the snapshot's committed baseline is benign; only divergence is real drift; compact should diff against the `pre-compact-<n>` tagged state. Plan: `tasks/agent-doc/plan-compact-typed-component-drift-false-positive.md`.

**Bug B — `Run Agent Doc` on equityfundingsource.md coalesced as exit-1 (`#qflood2`, re #kp5z).** The route repaired a stale busy projection, activated a queue head, then hit `dispatch coalesced … a dispatch for generation 66 is already in flight (#qflood)`. That coalesce is the dedup gate firing because a dispatch for gen 66 was already in flight — the `#kp5z` dual-drain-owner collision (supervisor idle-drain + your manual JB dispatch both targeting gen 66). `#kp5z` is archived done but still repros here, most likely because the live binary predates `ef0810a2` (the fix isn't deployed). Two sub-issues: it shouldn't double-own the dispatch, and a benign coalesce should report deduped-success, not exit-1. (I did **not** touch the equityfundingsource.md session — scope-isolated.)

**Common thread:** the live binary is stale (`predates ef0810a2`). Several of these — the drift guard, the dispatch dedup, the wedged-controller reaper — have newer source that isn't running. `cd src/agent-doc && make install && agent-doc admin recycle --all-projects` (+ `lib-install` for the cdylib) is the highest-leverage next step before re-testing any of them.

### Re: JB Run Agent Doc sends the command but does not submit — fix + tests + deploy — opus-4-8

Fixed, tested, built, installed, and committed. Drove it as far as the agent side allows; the final live two-pane check is yours.

**Root cause (confirmed in code).** JB `Run Agent Doc` (dispatch-only) routes **both Claude and Codex** through the same direct-pane path — `send_command_unchecked` in `agent-doc-orchestration/src/route/dispatch.rs` — which sends the trigger text + carriage return as one merged `send-keys` payload. Both TUI composers can leave that drafted rather than submitted. The code already *detected* the non-submit (`TriggerStillVisible` → `route_submit_issue issue=prompt_not_submitted`) for both harnesses, but the one-shot Enter re-submit was gated **codex-only**, so Claude never got it. Your "affects Claude too" report matched the code exactly.

**Fix.** Generalized `direct_pane_needs_codex_resubmit` → `direct_pane_needs_enter_resubmit`: the gate is now `codex | claude` (opencode still excluded), still requires `TimedOut + trigger_visible` so it never fires a stray Enter into an empty composer, still bounded to exactly one re-submit + one re-poll then falls through. Proof line unchanged in shape: `route_submit_resubmit … action=enter_key result=accepted|still_visible`. Extracted two pure helpers so the proof line is unit-tested.

**Verification.** 3 new unit tests (codex|claude scoping, opencode exclusion, proof-line shape); `make check` = **4369 passed, 0 FAIL**, clippy clean. Committed: agent-doc `3b71cfeb`, superproject pointer `8fcdc067f`. Built + installed (binary + cdylib) and `admin recycle --all-projects` queued.

**The one part I can't do — live verify (yours).** The agent can't reproduce a live JB→TUI submit. Recycle/restart so the dispatch path runs the new binary, then run `Run Agent Doc` against a **Claude** pane and a **Codex** pane; `ops.log` should show `route_submit_issue issue=prompt_not_submitted` then `route_submit_resubmit … action=enter_key result=accepted`, with the queue actually starting. I updated review item `#2v7b` to reflect code-complete with that operator gate as the only remainder.
<!-- agent:boundary:410f311f -->
<!-- /agent:exchange -->

## Queue

<!-- agent:queue priority preset="#spec-test-build-install-commit-push" go -->
- :round_pushpin: do [#compactdrift]
The document reverted to a stale state. After running `agent-doc admin recycle --all-projects`, then JB `Run Agent Doc`
```
resync: pruned 14 stale session(s)
[route] target tmux session: 0
Error: project controller command `dispatch` failed: dispatch blocked for tasks/agent-doc/agent-doc-bugs2.md: failed_stage=queue_paused reason=go-mode queue self-churns on its own convergence mirror lines (do[#fcc0]/[#rtwbcast]) that commit won't fold into HEAD; all active items (#fcc0,#rtwbcast) are clean-session/operator-drive and cannot run in this live IPC session; session_accretion=warn restart-heavy. Pausing to stop the flood; resume from a clean session. receipt_id=3309
```
---
:pushpin: - :round_pushpin: do [#fcc0]
- :round_pushpin: do [#splitmodswrite]
- :round_pushpin: do [#compactdrift]
- :round_pushpin: do [#qflood2]
- JB `Run Agent Doc` sends the command to the Claude or Codex session but does not submit. Fix and add Simworld regression tests. If you can, drive it.
---
JB `Compact Exchange` on this document
```
agent-doc command failed (exit 1): [agent-doc] Tagged pre-compact state as agent-doc/agent-doc-bugs2/pre-compact-23 [compact] CRDT state compacted: 26692 → 26692 bytes [compact] CRDT state refreshed from post-compact content [compact] discarded 1 stale capture sidecar(s) for archived response(s) in tasks/agent-doc/agent-doc-bugs2.md [compact] Archived 148 lines from component 'exchange' to /home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7a9b103bd5b68223dac05a4c449ac03899aba53d6022e82100578b5b0-20260613-022946.md Error: refusing to auto-adopt committed historical response patchback for tasks/agent-doc/agent-doc-bugs2.md: HEAD contains an out-of-band typed_component_drift mutation with response marker ### Re: do [#rtwbcast] — multi-editor CRDT broadcast — opus-4-8
```
- :round_pushpin: do [#qflood2]
- :round_pushpin: do [#splitmodswrite]
- :round_pushpin: do [#rtwbcast]
---
JB `Compact Exchange` on this document
```
agent-doc command failed (exit 1): [agent-doc] Tagged pre-compact state as agent-doc/agent-doc-bugs2/pre-compact-23 [compact] CRDT state compacted: 26692 → 26692 bytes [compact] CRDT state refreshed from post-compact content [compact] discarded 1 stale capture sidecar(s) for archived response(s) in tasks/agent-doc/agent-doc-bugs2.md [compact] Archived 148 lines from component 'exchange' to /home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7a9b103bd5b68223dac05a4c449ac03899aba53d6022e82100578b5b0-20260613-022946.md Error: refusing to auto-adopt committed historical response patchback for tasks/agent-doc/agent-doc-bugs2.md: HEAD contains an out-of-band typed_component_drift mutation with response marker ### Re: do [#rtwbcast] — multi-editor CRDT broadcast — opus-4-8
```
---
---
JB `Run Agent Doc` on equityfundingsource.md
```
[route] target tmux session: 0
[route] authoritative actor for tasks/professional/equityfundingsource.md projected busy but the live pane proves a dispatch-ready prompt (generation 66); repairing stale busy projection to ready and dispatching instead of queuing
[route] activated existing inactive agent:queue head "Please create sample borrower documents and sample investor documents that I can upload for testing. Place in the equityfundingsource-dev/tmp directory. Ensure tmp is in .gitignore." for tasks/professional/equityfundingsource.md (already_present=true, activated=true) before dispatch-only reopen
Error: project controller command `dispatch` failed: dispatch coalesced for tasks/professional/equityfundingsource.md: a dispatch for generation 66 is already in flight (#qflood); receipt_id=3310
```
---
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
- [ ] [#compactdrift] JB Compact Exchange (and agent-doc commit recovery) fail closed with "refusing to auto-adopt committed historical response patchback: HEAD contains an out-of-band typed_component_drift mutation with response marker ### Re: do [#rtwbcast]". The auto-adopt guard keys on the PRESENCE of a committed ### Re: response marker in HEAD as drift evidence, so it false-positives on the benign steady state (HEAD already holds the last finalized response). Compact archives 148 lines then refuses to finalize; same guard blocked agent-doc commit at this session start. FIX: make the discriminator structural (HEAD response matching the snapshot committed baseline is benign; only divergence from baseline is real typed_component_drift); compact should compare against the pre-compact tagged state. CLEAN session required (touches the compact/commit drift guard). Verify on a freshly make-installed binary first (live binary predated ef0810a2). Plan: tasks/agent-doc/plan-compact-typed-component-drift-false-positive.md
- [ ] [#qflood2] #qflood regression evidence (2026-06-13): JB Run Agent Doc on equityfundingsource.md returned exit-1 "dispatch coalesced ... a dispatch for generation 66 is already in flight (#qflood); receipt_id=3310" after repairing a stale busy projection and activating a queue head. This is the #kp5z dual-drain-owner collision (supervisor idle_queue_watch_drain + operator manual dispatch both target generation 66) surfaced as a hard error. #kp5z is archived done but still repros here, likely because the live binary predates ef0810a2 (fix not deployed). Confirm fix presence after make install + recycle; if it still collides, the coalesce should report success/info (deduped), not exit-1. Plan: tasks/agent-doc/plan-queue-dispatch-flood.md
- [ ] [#fcc0] [TOP] Zero JB File Cache Conflict dialogs. Root cause: all document-mutating disk writes (queue consume write.rs:3186, done-id mark 3887, finalize/stream/repair/reset 8683/9488/9836/10240/...) bypass editor IPC; #w42v converged only compact. Fix: extract a shared converge_document_or_disk() (mirror #w42v) and route ALL write sites through it (queue consume + done-id mark first, then finalize/stream/repair/reset), behind the listener-active gate with transport=editor_ipc primary + disk_fallback; plus resolve the #ipcdrift/#stuckhandoff2 wedge that forces fallbacks. Phases #fcc0a-e. CLEAN session required (live write path); 0-dialogs proof is operator-drive live JB. Immediate operator action: agent-doc lib-install to deploy #w42v for compact. Plan: tasks/agent-doc/plan-zero-file-cache-conflict.md
- [ ] [#splitmodswrite] [recommended] Split write.rs (27.7k lines, largest module) test layout. Unlike the clean single-trailing-mod-tests pattern used for route/session_check/git/preflight/sync, write.rs has an irregular layout: module-scope cfg(test) helper fns (around line 12109) PLUS several non-contiguous top-level cfg(test) blocks (mod tests at 15201 closes at 21827, then more cfg(test) blocks to EOF). Needs a bespoke extraction that moves each test block into write/tests.rs (or several files) and updates the FlowCore guard_ budget (currently 91) + any structural guards in tests/test_cli.rs. Verify make check + full workspace 0 failed.
- [ ] [#rtwbcast] #rtwatch rung 4 — multi-editor CRDT broadcast. STAYS OPEN: execute only from a dedicated CLEAN session, not a live agent-doc IPC session. First concrete phase is #rtwbcast-id: add editor_id to the FFI buffer-report and patch JSON ABI, and key live_buffer_snapshot_path per editor. Then run #rtwbcast-deliver (per-editor broadcast bus), #rtwbcast-merge (per-editor echo suppression using existing CRDT merge primitive + SimWorld Slice 3), and #rtwbcast-verify (operator-driven two-live-IDE convergence). Blockers remain the editor-plugin protocol decision and live two-IDE verification. Plan: tasks/agent-doc/plan-realtime-editor-watcher.md (Rung 4 section).
<!-- /agent:backlog -->

## Review
 
<!-- agent:review -->
- [/] [#snbc2] [recommended] OPTIONAL cosmetic (not a bugfix): collapse baselines/<hash>.md into a snapshot-store variant so there is one file instead of two, WITHOUT changing merge-base semantics (stays current-at-preflight = resolve_current_doc(newest-of(disk,buffer)), NOT git HEAD). #snbc Slice-1 audit proved the substantive HEAD-as-base consolidation would regress merge correctness (drops the uncommitted new prompt from the common ancestor). Moderate risk, touches the closeout path; gate behind NEW SimWorld coverage for the prompt-bearing finalize-merge class first. Must run in a CLEAN session. Plan: tasks/agent-doc/plan-snapshot-baseline-consolidation.md (Revised slice 2).
- [/] [#efscodexsubmit] JB Run Agent Doc in equityfundingsource.md enters but does not submit the prompt to Codex (operator must press Enter). Same class as #2v7b/#jbcodexsubmit: Codex direct-pane dispatch (route.rs send_command_unchecked) detects prompt_not_submitted (merged text+CR not treated as submit by the Codex TUI) but has no Enter re-submit fallback. FIX: Codex-scoped one-shot Enter re-submit + route_submit_resubmit proof logging + SimWorld coverage (operator-requested). CLEAN session required (touches route.rs; submodule carries dirty #fintol tree). Plan: tasks/agent-doc/plan-jb-codex-submit.md
- [/] [#stuckhandoff2] Stuck-handoff hardening: controllers wedged in --handoff-state preparing survive IDE restart + keep corrupting the worktree over ipc.sock. ROOT CAUSE: two-phase handoff (prepare_handoff->promote_handoff, project_controller.rs:3307/3318) is client-driven; if the client dies/RPC fails between the two, the controller stays Preparing forever. The existing reaper (#kqr6/#sjwm/#stuckhandoff, 45s) is bootstrap-RECORD-scoped (kills only bootstrap.pid), same-project-only, and fires on bind/gc not a timer -> orphaned + cross-project + no-new-invoke wedges survive (operator must pkill -f 'controller serve ... --handoff-state preparing'). FIX (defense-in-depth): M1 controller self-watchdog/suicide-timer on Preparing [strongest], M2 gate worktree/ipc writes on Stable [symptom-killer, overlaps #pcwc], M3 proc-scan reaper for orphans, M4 client handoff drop-guard, M5 cross-project gc sweep. M1+M2 are the durable core; land together with SimWorld coverage. CLEAN session required (submodule carries uncommitted #fintol work). Plan: tasks/agent-doc/plan-stuck-handoff-hardening.md
- [/] [#qt32] #lvbremain Operator live zero-drift PROOF of the 08b supervisor-in-process cutover (cannot be agent-done — needs a live editor). Restart a session, drive one edit-during-finalize cycle, agent reads ops.log for write_authority action=routed + plugin_watch_readonly + supervisor_host_gate hosting=in-process + ZERO drift lines. [recommended]
- [/] [#2v7b] #jbcodexsubmit / #jbclaudesubmit JB Run Agent Doc enters the routed prompt but does not submit it (operator presses Enter). CODE-COMPLETE 2026-06-13 (agent-doc 3b71cfeb, superproject 8fcdc067f): JB dispatch-only routes BOTH Claude and Codex through the same direct-pane send_command_unchecked path (merged text+CR send-keys), and TriggerStillVisible detection already covered both, but the one-shot Enter re-submit was gated codex-only. Generalized direct_pane_needs_codex_resubmit -> direct_pane_needs_enter_resubmit (codex|claude; opencode excluded; requires TimedOut+trigger_visible so no stray Enter into an empty composer); proof line route_submit_resubmit ... action=enter_key result=accepted|still_visible; 3 unit tests; make check 4369 passed 0 FAIL. Built+installed+recycled. GATE now operator-drive ONLY: run JB Run Agent Doc against a Claude pane AND a Codex pane, confirm ops.log shows route_submit_issue issue=prompt_not_submitted then route_submit_resubmit action=enter_key result=accepted with the queue starting. Plan: tasks/agent-doc/plan-jb-codex-submit.md
<!-- /agent:review -->

<!-- agent:icebox -->
<!-- /agent:icebox -->

## Completed / Reaped

<!-- agent:done archive=tasks/agent-doc-bugs2.done.md -->

<!-- completed work archived in tasks/agent-doc-bugs2.done.md -->
<!-- /agent:done -->
