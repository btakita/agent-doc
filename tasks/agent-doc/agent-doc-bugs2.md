---
agent_doc_session: b26b9957-8de6-4eae-bb98-5d7fe4d6b781
agent: claude
claude_model: opus
agent_doc_format: template
agent_doc_write: crdt
claude_args: --dangerously-skip-permissions
codex_args: -s danger-full-access
pending_done_guard: warn
prompt_presets:
  '#advance-review': Go through the review items. Add actionable backlog items to complete or advance the review items.,
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

completed
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7a9b103bd5b68223dac05a4c449ac03899aba53d6022e82100578b5b0-20260618-081944.md`*

Compacted content:
- Archived 11 response topic(s): #monster60stimeout route ready-prompt barrier; #describe-image-workflow; IPC drift root causes + restart-supervisor directive; 8 more
- Prior summary/context: prior compacted content: Archived 7 response topic(s): JB Clear Session Context on OpenCode does not submit `/new`; O...

### Re: JB Restart Supervisor → "Run Agent Doc" dispatch error (qflood queue pause) — opus-4-8

> **Queue prompt:**
>
> :pushpin: agent-doc should not thrash when the document changes during a turn...when the current document does not match the snapshot. The binary should gracefully resolve the difference without the agent knowing about it. Only structural conflicts based on the current work should be surfaced...such as editing the current prompt. Unrelated changes should be gracefully handled by the agent-doc binary without the agent needed to intervene.


> **Queue prompt:**
>
> do [#docdriftgrace]
>
> do [#qpauseux]


> **Queue prompt:**
>
> :pushpin: After a JB `Restart Supervisor Process` which surfaced a `Interrupt and Clear`...JB `Run Agent Doc` error
> ```
> resync: pruned 5 stale session(s)
> [route] target tmux session: 0
> Error: project controller command `dispatch` failed: dispatch blocked for tasks/professional/equityfundingsource.md: failed_stage=queue_paused reason=qflood: supervisor idle-watch is re-injecting agent-doc triggers on an already-drained queue (4 consecutive no-op /agent-doc cycles). All real work shipped+deployed (c9aa3a6 / worker fb99f394); only the gated #5eq8 (design-blocked) remains, which must not auto-drain. Leaving paused until operator adds new work / un-gates #5eq8. receipt_id=3933 ui_outcome_contract=ui-outcome-v1 ui_outcome=blocked_with_exact_unblocker ui_outcome_class=blocked next_action=follow_unblocker unblocker=resume_or_clear_queue_control trigger_bytes=52 trigger_sha256=a4b904c65fa35ab9631efaf329f3f2b595b448d1976b7c8738c6ba9b4a3af3c9
> ```


That error is the **qflood pause protection working as intended on `equityfundingsource.md`**, not a new failure. After your `Restart Supervisor Process`, "Run Agent Doc" tried to dispatch to `equityfundingsource.md`, whose queue is **paused** (`failed_stage=queue_paused reason=qflood`). The queue was auto-paused because the supervisor idle-watch was re-injecting `agent-doc` triggers on an already-drained queue (4 consecutive no-op cycles) — this is exactly **`#rz3a`** (the analyzed idle-watch re-injection bug, still gated DO-NOT-IMPLEMENT per its own directive). All real `equityfundingsource` work already shipped+deployed (`c9aa3a6` / worker `fb99f394`); only the gated `#5eq8` (design-blocked) remains, which must not auto-drain. So the dispatch is correctly **blocked, not broken** — the error even names the fix: `next_action=follow_unblocker unblocker=resume_or_clear_queue_control`.

**To clear it** (run against `equityfundingsource.md` — I'm not touching that doc from this agent-doc-bugs session): `agent-doc admin queue resume tasks/professional/equityfundingsource.md` if you want to add new work, un-gate `#5eq8`, or `agent-doc admin queue clear tasks/professional/equityfundingsource.md` to drop the paused control entirely.

**Real UX gap — filed `#qpauseux` [recommended].** A `queue_paused`/qflood dispatch block surfaces in JB "Run Agent Doc" as a raw red `Error:` toast rather than an actionable "queue paused — resume to continue" state. The unblocker token is in the text, but the framing reads as a crash. The plugin should render a paused-queue block as an info/action notification (with a Resume button) keyed off `ui_outcome=blocked_with_exact_unblocker unblocker=resume_or_clear_queue_control`.

**`#routedrainnextaction` is now live.** The supervisor you restarted came up on the pre-fix binary (preflight flagged `stale_install`), so I ran `make install`: binary + cdylib rebuilt at `a24ff990` (CI green on that push), and `lib-install` marked 2 controllers to auto-recycle at the next idle boundary. After this turn the fixed route-closeout diagnostic (`recovery_command=agent-doc reset --from-current ... / write --commit ...` instead of the misleading `wait_for_owner_turn_to_drain`) is running.

### Re: queue thrash after JB Clear Session + #jbacceptwedge 0.2.170 installed — opus-4-8

**#jbacceptwedge — PROVEN, marking done.** You installed 0.2.170; I checked the logs as agreed. `ops.log` has `ipc_accept_thread_spawned inflight=2` (3 occurrences). Under the old single-threaded accept loop `inflight` could never exceed 1 — peak ≥2 proves the per-connection-thread FFI fix (`86c68db7`) is live and handling concurrent connections instead of serializing them in the accept backlog. Live gate satisfied (operator-install ✅ + log proof ✅), so `#jbacceptwedge` is closed this cycle.

**The "After JB Clear Session" head is the qflood thrash, not new work.** Its own embedded route diagnostic says it: `failed_stage=queue_paused reason=qflood: degraded editor IPC (ack status error) keeps reverting agent-side queue strikes, re-presenting already-addressed stale heads (thrash/#docdriftgrace, File Cache Conflict/#pcwcdiskfree) as new prompts -> infinite no-op /agent-doc re-dispatch`. Every remaining drainable head maps to an item that is already addressed or operator-gated:

- **doc-thrash / "should not thrash when the document changes"** → already filed as `#docdriftgrace` (operator-verify, plan `plan-graceful-doc-drift-resolution.md`). Needs a live operator-watched re-baseline; agent cannot self-verify.
- **File Cache Conflict / "replace all direct file writes"** → tracked as `#pcwcdiskfree` (hot-path write authority). Operator-verify.
- **`#tsiftmdcrash`** → still blocked on you pasting the exact crash error/pane capture; logs show no panic signature, only the intended execve-recycle window. Not reproducible from logs alone.
- **`#qpauseux`** → filed (operator plugin-install live gate).
- **equityfundingsource.md / monsterrodholders.md heads** → cross-document. Out of scope for this `agent-doc-bugs2` session; I won't touch those docs from here (scope discipline). Their drift is the same class as `#docdriftgrace`.

So nothing here is new agent-drainable work — it's the documented qflood re-presentation pattern (`recent_noop_closeouts=5`). I'm closing `#jbacceptwedge`, reporting the rest, and **not** auto-looping these stale/operator-verify/cross-doc heads (looping would just churn another no-op cycle). The remaining backlog is `#docdriftgrace`, `#pcwcdiskfree`, `#tsiftmdcrash`, `#qpauseux` — all operator-verify; review items `#restartagentmenu`, `#agentreloadrestart`, `#rz3a` need focused clean cycles / are DO-NOT-IMPLEMENT-gated.

### Re: #tsiftmdcrash re-presented — filing the qflood root cause + pausing the flood — opus-4-8

`[#tsiftmdcrash]` came back to the queue, but nothing changed: it's still the same operator-verify item **blocked on you pasting the exact crash error text / pane capture / cycle id**. I can't `--done` it (not reproducible from logs — only the intended execve-recycle window appears) and `queue consume` refuses it (id-backed). This is the 6th consecutive no-op closeout — pure qflood thrash.

**Root cause filed → `#qheadsync` (plan `tasks/agent-doc/plan-answered-queue-head-autostrike.md`).** The thrash is structural: a closeout that *answers* a queue head never reconciles that resolution back to the head itself.
- **Free-text reports stay drainable after filing.** Your `File Cache Conflict` and `eqf/monsterrodholders drift` reports were filed as `#pcwcdiskfree` / `#docdriftgrace` (operator-verify), but the original free-text queue heads were never linked or struck → they re-present as `queue_drainable` every cycle.
- **`--done <id>` orphans the `[#id]` queue head.** Reaping the backlog item leaves the matching `[#id]` head in place → hard-blocks the next preflight and needs manual Edit+`reset`+`commit` surgery (hit this with `#jbacceptwedge` last cycle).

The fix (in the plan): `--done`/`--pending-gate` strikes the matching `[#id]` head in-write; filed free-text heads get linked to their backlog id so they defer like the item; drainability dereferences bare `[#id]` heads to their operator-verify tag.

**Halting the flood.** Until `#qheadsync` lands, every `/agent-doc` here is a no-op churn cycle. I'm pausing this queue and **stopping** — not auto-looping. Resume when you have new work, the `#tsiftmdcrash` crash text, or after `#qheadsync` is implemented. Note: editor IPC was degraded this whole session (commit refused twice, buffer mangled) so this response was written via `--force-disk`; your IntelliJ buffer will reload from disk on next focus.

### Re: automate the wedge clear — shipped (0.34.4) — opus-4-8

Done — the wedge now self-clears. Shipped agent-doc **0.34.4** (`08f026c0`, superproject `c47c89f71`, installed + CI green).

**What the wedge was:** after an IPC `content_ours` merge, `session-check` stayed `INTERRUPTED` forever on dropped queue edits `[#qpauseux]`/`[#docdriftgrace]` even though both were preserved in HEAD as struck `~~do [#id]~~` heads from a prior cycle. No `reset` variant cleared it — the only escape was a manual supervisor restart / patch surgery. Root cause: `check_dropped_queue_prompt_guard` proved preservation only by normalized **text identity** (which can't bridge the recorded bare `[#id]` against HEAD's `do [#id]`/struck spelling) and by a consumed-check scoped to **this cycle's** resolved ids (the strike happened earlier).

**The fix:** the guard now also clears when the dropped prompt's `[#id]` is present in the committed/visible `agent:queue` in **any** form, including struck/consumed lines — new `queue_ids_including_struck` strips the strike wrapper before id extraction. A struck head visibly reached the document, so its id is preserved, not silently lost. Genuinely lost ids (absent from the queue entirely) still fail closed. **Proven on this doc:** `session-check` now returns `ok — committed` instead of the `[#qpauseux]; [#docdriftgrace]` interruption.

**Your new report — "an equityfundingsource.md turn deleted what I wrote in the queue"** — is this same `#queue-user-edit-overwrite` drop. 0.34.4 makes `session-check` auto-**recover** from it (no more wedge / manual restart). But the deeper **prevention** is still open under `#qheadsync` (plan `plan-answered-queue-head-autostrike.md`): stop recording the false drop in the first place, strike the `[#id]` head on `--done`, and link filed free-text heads to their backlog id so they defer instead of re-presenting. The queue stays **paused** (`receipt_id=62`) until that prevention lands or you add new work; resume with `agent-doc admin queue resume tasks/agent-doc/agent-doc-bugs2.md`.
<!-- agent:boundary:41a39ca9 -->
<!-- /agent:exchange -->

## Queue

<!-- agent:queue priority preset="#spec-test-build-install-commit-push" priority go -->
~~~done
:pushpin: After a JB `Restart Supervisor Process` which surfaced a `Interrupt and Clear`...JB `Run Agent Doc` error
```
resync: pruned 5 stale session(s)
[route] target tmux session: 0
Error: project controller command `dispatch` failed: dispatch blocked for tasks/professional/equityfundingsource.md: failed_stage=queue_paused reason=qflood: supervisor idle-watch is re-injecting agent-doc triggers on an already-drained queue (4 consecutive no-op /agent-doc cycles). All real work shipped+deployed (c9aa3a6 / worker fb99f394); only the gated #5eq8 (design-blocked) remains, which must not auto-drain. Leaving paused until operator adds new work / un-gates #5eq8. receipt_id=3933 ui_outcome_contract=ui-outcome-v1 ui_outcome=blocked_with_exact_unblocker ui_outcome_class=blocked next_action=follow_unblocker unblocker=resume_or_clear_queue_control trigger_bytes=52 trigger_sha256=a4b904c65fa35ab9631efaf329f3f2b595b448d1976b7c8738c6ba9b4a3af3c9
```
~~~
- ~~do [#docdriftgrace]~~
- ~~:pushpin: do [#routedrainnextaction]~~
- ~~:pushpin: agent-doc should not thrash when the document changes during a turn...when the current document does not match the snapshot. The binary should gracefully resolve the difference without the agent knowing about it. Only structural conflicts based on the current work should be surfaced...such as editing the current prompt. Unrelated changes should be gracefully handled by the agent-doc binary without the agent needed to intervene.~~
- [#tsiftmdcrash]
- :pushpin: An equityfundingsource.md turn had a wedge and deleted what I wrote in the queue.
- :pushpin: JB `File Cache Conflict` still occurring. Please replace all direct file writes on the hot path.
---
:pushpin: JB `Run Agent Doc` on equityfundingsource.md
```
[route] target tmux session: 0
[preflight] review: 14 item(s), 14 gated
[route] active closeout for tasks/professional/equityfundingsource.md could not be drained before reroute; existing queue head "Why is the https://equityfundingsource.brian-takita.workers.dev/admin/pipeline?dialog=deal&dialog_deal_id=7a0c1463-27bb-4d7c-82f7-2281b3c008d3&deal_tab=application > Available deal > Application > Application progress > Accepted not checked?" remains queued behind the closeout ui_outcome_contract=ui-outcome-v1 ui_outcome=queued_behind_owner ui_outcome_class=ok next_action=wait_for_owner_turn_to_drain
```
---
JB `Run Agent Doc` on equityfundingsource.md
```
resync: pruned 5 stale session(s)
[route] target tmux session: 0
Error: project controller command `dispatch` failed: dispatch blocked for tasks/professional/equityfundingsource.md: failed_stage=queue_paused reason=qflood: supervisor idle-watch is re-injecting agent-doc triggers on an already-drained queue (4 consecutive no-op /agent-doc cycles). All real work shipped+deployed (c9aa3a6 / worker fb99f394); only the gated #5eq8 (design-blocked) remains, which must not auto-drain. Leaving paused until operator adds new work / un-gates #5eq8. receipt_id=3934 ui_outcome_contract=ui-outcome-v1 ui_outcome=blocked_with_exact_unblocker ui_outcome_class=blocked next_action=follow_unblocker unblocker=resume_or_clear_queue_control trigger_bytes=52 trigger_sha256=a4b904c65fa35ab9631efaf329f3f2b595b448d1976b7c8738c6ba9b4a3af3c9
```
- ~~[#jbacceptwedge]: 0.2.170 installed~~
- :pushpin: equityfundingsource.md & monsterrodholders.md keeps on drifting with edits.
- ~~do [#qpauseux]~~
<!-- /agent:queue -->
<!--
Please make backlog replacements for all gated review items. We need to complete the plan, not have any more gates after this round. Let me know when I need to install JB plugin updates. Make sure all JB live gate items have adequate logging to prove/disprove the item. Clear the review items. I will test, files bugs, and you will check the logs.
---
`'/clear' is disabled while a task is in progress
---~~~~
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
The last run had a code fence stripped away by agent-doc.
#spec-test-build-install-commit-push
---
What are #next-steps to fix bugs?
-->

## Backlog

<!-- agent:backlog priority queue -->
- [ ] [#qpauseux] [operator-verify] [recommended] JB Run Agent Doc surfaces a queue_paused/qflood dispatch block as a raw red Error: toast instead of an actionable queue paused — resume notification. Plugin should render a paused-queue block (ui_outcome=blocked_with_exact_unblocker unblocker=resume_or_clear_queue_control) as an info/action notification with a Resume button rather than a dispatch error. Operator plugin-install live gate.
- [ ] [#docdriftgrace] [operator-verify] agent-doc thrashes when the session doc changes mid/between turn in ways unrelated to the current prompt: snapshot>visible drift (operator trimmed old committed Re: blocks + harness switch + queue edits) made preflight refuse commit (looks like a manual cleanup with stale snapshot) and forced a manual reset --from-current. Binary should classify drift by component and silently auto-rebase snapshot/CRDT for unrelated changes, surfacing ONLY structural conflicts on the in-flight prompt/response. Needs operator-watched live re-baseline. Plan: tasks/agent-doc/plan-graceful-doc-drift-resolution.md
- [ ] [#tsiftmdcrash] [operator-verify] Operator reported 'tsift.md agent-doc turn crashed and killed the session process. The tmux pane was still active.' Supervisor pid 1742954 is alive now; pane %78 OpenCode TUI shows 'Thinking' normally; tsift-v0.1.log (994KB) shows NO panic/abort/crash signatures — only two intentional supervisor_binary_stale_self_recycled via=execve_preserve_child entries from my cargo install + make install (#supkill-bg blue/green drain-and-supersede). Most plausible: operator saw the brief execve-recycle window (current_exe=(deleted)) as a crash. NEED operator to paste exact error text or the pane capture from the crash moment, or the specific cycle id, to root-cause. Not currently reproducible from logs alone.
<!-- /agent:backlog -->

## Review
 
<!-- agent:review -->
- [/] [#restartagentmenu] Plan tasks/agent-doc/plan-restart-agent.md Phase 2 (after #agentreloadrestart). Add a Restart Agent JB plugin menu command: new RestartAgentAction.kt mirroring RestartSupervisorProcessAction.kt, registered in plugin.xml beside AgentDoc.RestartSupervisorProcess, wired through the TerminalUtil restart path. After Phase 1 re-resolves the harness on restart, this command brings up a changed agent. Bump pluginVersion (gradle.properties, currently 0.2.170), build unsigned + signed zips. Decide alongside vs replace the existing Restart Supervisor Process action (do not remove without operator confirmation). Operator-install live gate.
- [/] [#agentreloadrestart] Plan tasks/agent-doc/plan-restart-agent.md Phase 1 (foundational). Make the supervisor restart path re-resolve the harness from current frontmatter so changing agent (claude to opencode) takes effect on restart, and auto-restart the agent at the next idle/turn boundary when the resolved harness differs from the running one. Today the restart loop reuses the harness resolved once at start (SharedSupervisorState.harness, start.rs:171) and never re-reads fm.agent, so restart-supervisor restarts the SAME harness. Re-resolve via HarnessConfig::from_context (harness.rs:116) before respawn, rebuild child_factory/harness_binary, then gate the boundary auto-restart exactly like the supselfheal auto-recycle (prompt_visible and not turn_active, debounced, never mid-turn) behind a new agent_doc_agent_change_restart knob (default ON). SimWorld tests + SPEC + VERSIONS. Live-supervisor-critical: needs a focused clean cycle with make check + make tmux-ci.
- [/] [#rz3a] qflood-gated supervisor idle-watch re-injection: the supervisor idle-queue watch keeps re-injecting agent-doc FILE triggers on a queue whose only remaining head is gated / operator-blocked, causing repeated no-op agent-doc cycles. REPRO (equityfundingsource.md, 2026-06-18): a go-queue block was fully drained (all 7 free-text heads consumed via agent-doc queue consume); the lone remaining head (a do-reference to a now-gated review item) was gated via pending-gate and moved to agent:review. session-check then reported queue_continuation_required=false, queue_drainable_head_count=0, ui_outcome=no_drainable_work. After agent-doc admin queue resume, the supervisor idle-watch re-injected agent-doc FILE repeatedly: 4 consecutive no-op agent-doc cycles, each preflight no_changes=true. agent-doc admin queue pause stops it. EXPECTED: when queue_drainable_head_count==0 and queue_continuation_required==false (only gated or operator-verify heads remain), the supervisor idle-queue watch should DEFER instead of injecting, matching the in-session loop stop rule. ROOT-CAUSE HYPOTHESIS: the idle-watch dispatch gate keys off queue presence / queue:start frontmatter instead of the binary drainability filter; the in-session loop already honors queue_continuation_required=false but the supervisor idle-watch path does not. FIX DIRECTION (do not implement): make the supervisor idle-queue watch reuse the same drainability filter as the in-session loop and defer when queue_drainable_head_count==0. DO NOT IMPLEMENT THE FIX.
<!-- /agent:review -->

<!-- agent:icebox -->
<!-- /agent:icebox -->

## Completed / Reaped

<!-- agent:done archive=tasks/agent-doc-bugs2.done.md -->

<!-- completed work archived in tasks/agent-doc-bugs2.done.md -->
<!-- /agent:done -->
