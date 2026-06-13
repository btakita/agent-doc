---
agent_doc_session: b26b9957-8de6-4eae-bb98-5d7fe4d6b781
agent: claude
claude_model: opus
agent_doc_format: template
agent_doc_write: crdt
claude_args: --dangerously-skip-permissions
codex_args: -s danger-full-access
prompt_presets:
  '#advance-review': Go through the review items. Add actionable backlog items to complete or advance the review items.,
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

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7a9b103bd5b68223dac05a4c449ac03899aba53d6022e82100578b5b0-20260613-080812.md`*

Compacted content:
- Archived 4 response topic(s): Triage of the queued JB bug reports + #fcc0/#compactdrift/#fccsupwarn; Re-run; #fccsupwarn shipped + supervisor automation live; 1 more
- Prior summary/context: ### Session Summary ❯ *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039...

### Re: Triage of the queued JB bug reports + the live "prompt removed then came back" root cause — opus-4.8

I found and fixed the live root cause of your headline bug this cycle, and triaged the rest. **No code changes** — every actionable item here is `[clean-session]` + operator-drive, and this is a live route-owned session (supervisor `1075224`), which is exactly the session class those items must NOT be worked in.

**0. Live root cause of "agent-doc removed my prompt, then the deleted prompt came back from a stale HEAD" — FOUND + FIXED this cycle.** A `controller serve … --handoff-state preparing` controller (PID `2434403`, generation 1060, superproject root) had been **wedged for 13.5 minutes** (handoffs are sub-second). That is the live `#stuckhandoff2` wedge: an orphaned preparing controller survives, keeps writing the stale buffer over `ipc.sock`, and resurrects content from a stale HEAD — precisely your symptom. I `kill`ed it. I then had to recover this doc twice (the `#exch-intermix` replay spliced my response into the summary bullets on the first IPC write), clearing a bloated 42KB stale CRDT overlay and rebuilding from disk.

**1. Every queued bug report is already tracked — mapping:**
- *"JB File Cache Conflict dialogs, should be 0"* -> **#fcc0** (backlog TOP, clean-session; 0-dialogs proof is operator-drive live JB).
- *"Compact Exchange has a partially uncommitted response"* -> **#compactdrift** (backlog; the `typed_component_drift` false-positive — reproduced above when I recovered this very doc, HEAD held the uncompacted exchange).
- *"Clear Exchange content came back from a stale HEAD"* -> same `#compactdrift` + the **#stuckhandoff2** stale-buffer resurrection I just killed.
- *"agent-doc removed my prompt, then it came back"* -> the **#stuckhandoff2** wedge (item 0). The 13.5-min orphan is direct evidence the existing reaper (#sjwm/#kqr6) is bootstrap-scoped and misses orphaned/no-new-invoke wedges, so M1 (controller self-watchdog suicide-timer on `Preparing`) + M3 (orphan proc-scan reaper) are still needed.
- *"Run Agent Doc sends but does not submit"* -> **#2v7b / #jbclaudesubmit** (review; CODE-COMPLETE — generalized `direct_pane_needs_enter_resubmit` covers claude|codex). GATE is operator-drive: run it against a Claude pane, confirm ops.log shows `route_submit_issue issue=prompt_not_submitted` then `route_submit_resubmit action=enter_key result=accepted`.
- *"tsift.md queue struck the next open item after I deleted struck ones"* -> **#goqueuestall** (done) / **#goqstall2**.

**2. Semantic-completion signals — my read (mostly false positives, do not auto-close):**
- **#stuckhandoff2 ~ #sjwm** -> **false positive, keep open.** I just live-proved a 13.5-min orphan #sjwm's reaper did not catch.
- **#efscodexsubmit ~ #fintol** -> false positive on #fintol, but likely **covered by #2v7b's** Enter-resubmit generalization — verify in the same operator pass as #2v7b.
- **#snbc2 ~ #snbc** -> keep (optional cosmetic remainder, distinct scope).
- **#rtwbcast ~ #rtwatch** -> keep (rung 4 builds on #rtwatch).
- **#goqstall2 ~ #goqueuestall** -> **appears satisfied** — preflight this cycle logged `skipped 4 agent-undrainable backlog head(s) … #goqueuestall`, exactly #goqstall2's ask. Recommend you confirm and close; I'm not closing a `[clean-session]` item from a live session unilaterally.

**3. A new lint bug surfaced this cycle (worth a backlog item).** finalize's lint-gate flagged the `&lt;!-- agent:queue --&gt;` markers **inside your fenced code examples** (the tsift.md bug report) as real `duplicate-component` opens (`agent-doc/duplicate-component` at the fenced lines). Fenced code must be excluded from component-structure linting. I committed this response with `--lint off`.

**4. Why no auto-loop / no code.** The queue head `do [#fcc0]` and the other `do[#id]` heads were correctly skipped as agent-undrainable (clean-session under live IPC / operator-verify) — go-mode behaved. The remaining items are bug reports, all tracked. Looping or forcing clean-session code here would churn no-op cycles and risk re-corrupting the buffer (which it just did, twice).

**To make progress, operator-drive:** (a) `cargo build --release && agent-doc lib-install` + `admin recycle --all-projects` so the committed #jbclaudesubmit/#qflood2/#goqueuestall fixes go live in JB, then re-test #fcc0 and #2v7b and I'll check the logs; (b) tell me to take **#stuckhandoff2 M1+M3** (the durable orphan-reaper I just had to run by hand) in its own clean session — the submodule is clean now, so it's unblocked.

### Re: "continue" → took #stuckhandoff2, and it is NOT actually complete (M1 self-watchdog is dead in production) — opus-4.8

I took the #stuckhandoff2 work you greenlit (submodule is clean, blocker resolved). Investigating it overturned the plan's "complete" claim — with hard telemetry.

**What the plan claims:** M1 (controller self-watchdog suicide-timer) makes every `Preparing` controller self-terminate at the 45s threshold "with no dependency on an external reaper tick." M1–M5 all landed; "#stuckhandoff2 is complete."

**What production actually shows:**
- The wedged controller I killed this cycle (pid 2434403, gen 1060) survived **13.5 minutes**. The binary was installed `03:57:27`; that controller started `03:57:41` — 14s *after* install, so it ran the **fresh M1 binary**. Not a stale-binary artifact.
- `.agent-doc/logs/ops.log`: `stale_preparing_controller_self_reaped` (M1, `caller=self_watchdog`) — **zero occurrences, ever.**
- `orphaned_preparing_controller_reaped_cross_project … caller=gc` — fires **repeatedly** (pids 401211, 1151060, 1324526, 1663915, 2011236, 2214912, …) at ages **442–1286s**, threshold 45s.

**Conclusion:** M1 never fires at runtime. Orphaned `Preparing` controllers are cleaned up *only* by the slow external gc/M5 sweep — at 7–21 minutes, not 45s — i.e. exactly the external-reaper dependency the plan said M1 removed. M1 is present, unit-tested as a pure predicate, and reached by the launch path (`write_bootstrap_with_options` stamps `handoff_started_at`), yet the suicide never triggers. This **reopens #stuckhandoff2 (M1)**.

**I did NOT ship a fix.** I read-only-investigated and narrowed it to three hypotheses (serve loop never hits the `WouldBlock` watchdog branch / in-memory state flipped off `Preparing` while the `/proc` cmdline M5 keys on stays `preparing` / poll-cadence wiring), but I have not pinned the exact mechanism. Shipping a speculative edit to clean-session live-write-path serve-loop code — in a session that already corrupted twice this cycle, without a deterministic repro — would violate both the clean-session rule and precommit-testing discipline.

**Recorded** in `tasks/agent-doc/plan-stuck-handoff-hardening.md` (committed `7d9aeabd1`): the evidence, the three hypotheses, and a concrete next-cycle task — a deterministic test that drives a launched-`Preparing` controller's serve loop to the watchdog branch and asserts a self-reap within threshold, then fix whichever hypothesis holds. (Could not file this into `agent:backlog` — the doc's fenced `&lt;!-- agent:queue --&gt;` examples still break the granular-write parser, `#lintfence`.)

**Recommended next step:** give me a genuinely clean session (no live route-owned supervisor for this doc) and I'll write the repro test + fix M1 there. In the meantime the wedge window is gc-cadence (minutes); `admin recycle --all-projects` clears current orphans on demand.
<!-- agent:boundary:43254d6e -->
<!-- /agent:exchange -->

## Queue

<!-- agent:queue priority preset="#spec-test-build-install-commit-push" go -->
- :round_pushpin: do [#fcc0]
- :round_pushpin: do [#compactdrift]
- I'm still seeing JB `File Cache Conflict` dialogs. There should be 0.
- :pushpin: JB `Compact Exchange` has a partially uncommitted response.
- :pushpin: JB `Clear Exchange` cleared the exchange but the content came back...from a stale HEAD?
- I entered `I'm still seeing JB `File Cache Conflict` dialogs. There should be 0.` before and agent-doc removed it. I had to type in the prompt again. Then the deleted prompt came back. So I'm leaving this queue item here so you can see the bug.
- :pushpin: JB `Run Agent Doc` sends the command to the Claude or Codex session but does not submit. Fix and add Simworld regression tests. If you can, drive it.
---
:pushpin: In tsift.md, I had a queue:
```
<!-- agent:queue preset="#spec-test-build-install-commit-push" go -->
- ~~[#pcacheattr]~~
- ~~[#gdbwritelock]~~
- ~~[#pcacheregtrunc]~~
- [#loggatefidelity]
- [#gdbwalcheckpoint]
- [#logfoldname]
- [#pcachebp]
<!-- /agent:queue -->
```
I then manually deleted the strikethrough items...but then agent-doc struck through the next open item
```
<!-- agent:queue preset="#spec-test-build-install-commit-push" go -->
- ~[#loggatefidelity]~
- [#gdbwalcheckpoint]
- [#logfoldname]
- [#pcachebp]
<!-- /agent:queue -->
```
---
JB `Compact Exchange` on this document
```
agent-doc command failed (exit 1): [agent-doc] Tagged pre-compact state as agent-doc/agent-doc-bugs2/pre-compact-23 [compact] CRDT state compacted: 26692 → 26692 bytes [compact] CRDT state refreshed from post-compact content [compact] discarded 1 stale capture sidecar(s) for archived response(s) in tasks/agent-doc/agent-doc-bugs2.md [compact] Archived 148 lines from component 'exchange' to /home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7a9b103bd5b68223dac05a4c449ac03899aba53d6022e82100578b5b0-20260613-022946.md Error: refusing to auto-adopt committed historical response patchback for tasks/agent-doc/agent-doc-bugs2.md: HEAD contains an out-of-band typed_component_drift mutation with response marker ### Re: do [#rtwbcast] — multi-editor CRDT broadcast — opus-4-8
```
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
- [ ] [#goqstall2] [clean-session] go-mode drain + session-check queue_continuation_required do NOT defer pre-materialized undrainable do[#id] queue heads (clean-session/operator-verify backlog items) or stale console-output queue lines, so a queue populated before #goqueuestall churns no-op cycles instead of self-deferring. Resolve each materialized do[#id] head's backlog tag in the continuation head set + classify non-directive/console-output lines as noise; emit go_queue_skip_undrainable origin=materialized_head + queue_stale_noise_lines diagnostic. CLEAN session (touches live queue-continuation logic). Plan: tasks/agent-doc/plan-go-queue-stall-materialized-heads.md
- [ ] [#compactdrift] [clean-session] JB Compact Exchange and agent-doc commit recovery fail closed on the typed_component_drift auto-adopt false-positive (guard keys on presence of a committed Re: marker in HEAD). Make the discriminator structural; compact should diff the pre-compact tag. CLEAN session required. Plan: tasks/agent-doc/plan-compact-typed-component-drift-false-positive.md
- [ ] [#fcc0] [TOP] [clean-session] Zero JB File Cache Conflict dialogs. Route all document-mutating disk writes through one shared converge_document_or_disk() behind the listener-active gate (editor_ipc primary + disk_fallback); resolve the ipcdrift/stuckhandoff2 wedge that forces fallbacks. CLEAN session required (live write path); 0-dialogs proof operator-drive live JB. Plan: tasks/agent-doc/plan-zero-file-cache-conflict.md
- [ ] [#rtwbcast] [clean-session] rtwatch rung 4 — multi-editor CRDT broadcast. Phases rtwbcast-id (editor_id in FFI buffer-report + per-editor live_buffer_snapshot_path), then deliver/merge/verify. CLEAN session required; live two-IDE convergence verify is operator-drive. Plan: tasks/agent-doc/plan-realtime-editor-watcher.md
<!-- /agent:backlog -->

## Review
 
<!-- agent:review -->
- [/] [#qflood2] [operator-verify] qflood regression: JB Run Agent Doc on efs coalesced as exit-1 (dual-drain owner, gen 66 already in flight). Needs live multi-process verify; if it still collides, coalesce should report deduped-success not exit-1. Plan: tasks/agent-doc/plan-queue-dispatch-flood.md
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
