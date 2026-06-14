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
  '#simworld': Strengthen Simworld tests.
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

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7a9b103bd5b68223dac05a4c449ac03899aba53d6022e82100578b5b0-20260613-230948.md`*

Compacted content:
- Archived 14 response topic(s): Post-compaction recovery + no agent-drainable work remains; Compaction recovery + killed a live #stuckhandoff2 wedge; hot-reload stalling + recycle doesn't restart + Sync deferred; 11 more
- Prior summary/context: ### Session Summary ❯ *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039...
- `cargo build --release --lib` + `agent-doc lib-install` → cdylib refreshed (`libagent_doc-0.34.0.so`)
- `cargo install --path .` → `~/.cargo/bin/agent-doc` now at `ce339055` (v0.34.0)

### Re: post-compaction recovery + go-queue is in #qchurn no-op churn — opus-4-8

**Recovery done.** This session opened on a just-run compaction that was never committed: the working tree + snapshot held the compacted doc (~18.8KB, archive `…-20260613-230948.md`, 40KB) while HEAD was still the pre-compact 58.9KB version. Plain `agent-doc commit` would have auto-recovered and *un-compacted* (restored HEAD), so I used `reset --from-current --preserve-session` → `commit` (8c225bcbf). HEAD is now the compacted 18.8KB; working tree clean.

**No agent-drainable work this cycle.** Preflight skipped 12 agent-undrainable backlog heads — every id-backed `do #id` in the queue is `[clean-session]` (#qchurn, #qdedup, #clearexchstale, #qpresetstrike, #fcc0, #rtwbcast, #recyclerestart) or `[operator-verify]` (#c2b6, #aazp, #4myd). Clean-session items need a fresh session (not this live-IPC one); operator-verify items need a live editor. The free-text queue reports are already captured (#mdastquote/#0kjc, the JB Compact Exchange `typed_component_drift` error → plan-compact-typed-component-drift-false-positive.md, the efs dispatch-coalesce → #qflood2).

**This very cycle reproduces #qchurn:** go-mode's only diff was auto-appending `[#qchurn]` to the queue — a no-op churn with `user_intent_prompt_changes` empty and zero drainable work. Not auto-looping; leaving queue state untouched (live IPC supervisor). Operator drive needed to advance the remaining items.

### Re: queue reorder — qpresetstrike/clearexchstale/qdedup to top (still clean-session) — opus-4-8

You moved `#qpresetstrike`, `#clearexchstale`, `#qdedup` to the head of the go-queue and stripped their `do` markers (now plain `:pushpin: [#...]`). That's a reorder, not a new directive — and all three are tagged `[clean-session]` (lines 135–137), so go-mode correctly keeps deferring them. Pinning them to the top doesn't make them agent-drainable in this live-IPC session; they still need a fresh/clean session because the live supervisor races finalize writes. No agent-drainable head appeared this cycle (same as the prior closeout).

**The actionable item this cycle is the stale supervisor.** Preflight flagged that the route-owned host supervisor (pid 2825163) serving this doc started *before* the currently-installed binary — it's running a STALE build and is the root of the recurring File Cache Conflict / IPC-drift dialogs and the no-op churn (2 no-op closeouts in 30 min, `session_accretion: warn`). That's also why the clean-session queue can't drain: the old binary keeps re-racing.

**Recommended (operator-drive — restarting the live host supervisor is your call):**
```
agent-doc session restart-supervisor tasks/agent-doc/agent-doc-bugs2.md --force
```
After it comes up on the current binary, a clean session can actually drain the clean-session queue (#qpresetstrike / #clearexchstale / #qdedup all have plans under `tasks/agent-doc/plan-*.md`). I'm not restarting it mid-cycle unilaterally or auto-looping — clean-session heads aren't agent-completable here, and looping would just reproduce the churn.

### Re: JB `Run Agent Doc` should auto-restart the supervisor (churn-stop dispatch block) — opus-4-8

> **Queue prompt:**
>
> :pushpin: JB `Run Agent Doc`. This should automatically restart the supervisor.
> ```
> [route] target tmux session: 0
> Error: project controller command `dispatch` failed: dispatch blocked for tasks/agent-doc/agent-doc-bugs2.md: failed_stage=queue_paused reason=churn-stop: do[#c2b6] operator-verify head re-injected by stale supervisor pid1368698 (pre-0.34.0); needs operator recycle, not agent drain receipt_id=3420
> ```


Filed as a bug + plan, not fixed live (clean-session code path; restarting the live supervisor is operator-drive).

**What happened.** JB `Run Agent Doc` dispatched but failed closed:
`failed_stage=queue_paused reason=churn-stop: do[#c2b6] operator-verify head re-injected by stale supervisor pid1368698 (pre-0.34.0); needs operator recycle, not agent drain`. The churn-stop *reason text itself* prescribes the remedy ("needs operator recycle") — yet the dispatch path surfaces it as a terminal error and does nothing, so you'd have to recycle by hand.

**Root cause.** A stale supervisor re-injects an operator-verify head → churn detector PAUSES the queue (`queue_paused`/`churn-stop`). The supervisor self-recycle that already exists (`supervisor_binary_stale_self_recycled`, #aazp/#4myd) is gated on the **idle / next-queue-item** boundary — which a churn-stopped queue never reaches. So the recycle machinery is present but unreachable from a dispatch-blocked entry, and it wedges until you recycle manually. Same stale-binary root as #fcc0/#ipcdrift.

**The fix (queued, clean-session).** New backlog `#jbrestale` + plan `tasks/agent-doc/plan-jb-run-restart-stale-supervisor.md`: in the JB Run dispatch handler, classify a churn-stop whose reason is a stale supervisor, branch to `restart-supervisor --force` (proof `route_dispatch_recovery action=restart_supervisor … result=restarted|reexec_failed`), lift the stale-injected pause, re-dispatch **once** (no loop), keep operator-verify heads deferred (#goqueuestall), and on `reexec_failed` fail closed but keep the pane alive with the manual hint. SimWorld: stale-cause recovers once; non-stale churn still fails closed; reexec-failed stays alive. Touches route/dispatch + recycle → must land in a clean session.

**Immediate workaround (your call — I'm not restarting the live supervisor mid-cycle).** This session's host supervisor (`pid 2825163`) is itself stale (predates the installed binary), which is what's generating the churn:
```
agent-doc session restart-supervisor tasks/agent-doc/agent-doc-bugs2.md --force
```
Preflight also flags `stale_install` (`target/release/agent-doc` predates `ce339055`) — if you want the *restarted* supervisor on current code, run `make install` in `src/agent-doc` first, then restart. After that a clean session can actually drain the clean-session queue.

No agent-drainable work this cycle beyond filing this; not auto-looping (churn-stopped queue, live-IPC supervisor).
<!-- agent:boundary:9b5ff8c6 -->
<!-- /agent:exchange -->

## Queue

<!-- agent:queue priority preset="#spec-test-build-install-commit-push" go -->
---
:pushpin: JB `Run Agent Doc`. This should automatically restart the supervisor.
```
[route] target tmux session: 0
Error: project controller command `dispatch` failed: dispatch blocked for tasks/agent-doc/agent-doc-bugs2.md: failed_stage=queue_paused reason=churn-stop: do[#c2b6] operator-verify head re-injected by stale supervisor pid1368698 (pre-0.34.0); needs operator recycle, not agent drain receipt_id=3420
```
---
- ~~[#qchurn]~~
- :pushpin: [#qpresetstrike]
- :pushpin: [#clearexchstale]
- :pushpin: [#qdedup]
- :pushpin: :round_pushpin: do [#c2b6]
- :pushpin: :round_pushpin: do [#aazp]
---
:pushpin: In tsift.md, I had a queue:
```
&gt;!-- agent:queue preset="#spec-test-build-install-commit-push" go --&lt;
- ~~[#pcacheattr]~~
- ~~[#gdbwritelock]~~
- ~~[#pcacheregtrunc]~~
- [#loggatefidelity]
- [#gdbwalcheckpoint]
- [#logfoldname]
- [#pcachebp]
&gt;!-- /agent:queue --&lt;
```
I then manually deleted the strikethrough items...but then agent-doc struck through the next open item
```
&gt;!-- agent:queue preset="#spec-test-build-install-commit-push" go --&lt;
- ~~[#loggatefidelity]~~
- [#gdbwalcheckpoint]
- [#logfoldname]
- [#pcachebp]
&gt;!-- /agent:queue --&lt;
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
- :pushpin: :round_pushpin: do [#4myd]
- :pushpin: :round_pushpin: do [#fcc0]
- :pushpin: :round_pushpin: do [#rtwbcast]
- [#recyclerestart]: Stall occurred ![img_23.png](img_23.png)
- Ensure the markdown AST is properly handling strings (quoted text) and code blocks (backtick or triple backtick). Tags within the blocks should be treated as the content, not as part of the document tags.
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
- [ ] [#jbrestale] [clean-session] JB `Run Agent Doc` should auto-restart a stale/churn-stopped supervisor instead of failing closed. Operator pressed Run Agent Doc expecting a supervisor restart; dispatch returned `failed_stage=queue_paused reason=churn-stop: ... re-injected by stale supervisor pid1368698 (pre-0.34.0); needs operator recycle, not agent drain`. The churn-stop reason itself prescribes a recycle, but the dispatch path treats it as terminal. ROOT CAUSE: the supervisor self-recycle (`supervisor_binary_stale_self_recycled`, #aazp/#4myd) is gated on the idle/next-queue-item boundary, which a churn-stopped queue never reaches. FIX: in the JB Run Agent Doc dispatch handler, classify a churn-stop whose reason is a stale supervisor (`supervisor_binary_stale`/`pre-<ver>`) and branch to restart-supervisor (proof `route_dispatch_recovery action=restart_supervisor … result=restarted|reexec_failed`), lift the stale-injected pause, re-dispatch ONCE (no loop), keep operator-verify heads deferred (#goqueuestall), fail closed + keep pane alive on reexec_failed. CLEAN session (route/dispatch + recycle path). Related #recyclerestart/#aazp/#4myd/#qchurn/#fcc0. Plan: tasks/agent-doc/plan-jb-run-restart-stale-supervisor.md
- [ ] [#0kjc] #mdastquote [clean-session] Markdown AST must treat tags inside quoted strings ("...") and code spans/blocks (backtick / triple-backtick) as CONTENT, not document tags. Operator report: agent:*/#id-looking tokens inside quotes or fenced code are being parsed as real document component markers. Same family as #lintfence (fenced agent:* markers break lint + parser). FIX: tagpath/agent-doc markdown tokenizer must mask quoted-string and code-span/code-fence regions before component/tag extraction. CLEAN session (parser path).
- [ ] [#pushpinaccum] [clean-session] Queue `:pushpin:` annotations accumulate/nest in the wrong place: a head already starting `- :pushpin:` gets another `:pushpin:` prepended -> `:pushpin: - :pushpin: ...` (operator corrected one live 2026-06-13). The queue head-grouping/annotation formatter prepends the pushpin marker without checking for an existing leading pushpin/bullet. FIX: make the queue-head annotation prefix idempotent (single :pushpin:/bullet). CLEAN session (queue formatter path).
- [ ] [#c2b6] [operator-verify] compactdrift live JB Compact Exchange re-verify: with a freshly make-install-ed agent-doc (installed cdylib currently predates aafa6510), run JB Compact Exchange on a doc whose HEAD holds a finalized Re: response and confirm it compacts WITHOUT the "refusing to auto-adopt committed historical response patchback / out-of-band typed_component_drift" bail. Any persistent failure from a NON-wedged session files against stuckhandoff2, not here. Code complete + committed agent-doc bca26cfd; 108 commit_ tests green this cycle.
- [ ] [#aazp] [operator-verify] suprecyclestall live-verify (agent-doc aafa6510, installed+release-built): restart a stale session via session restart-supervisor FILE --force so it runs the new binary, then drive an idle/next-queue-item recycle. Confirm ops.log shows supervisor_binary_stale_self_recycled ... candidates=[...] then EITHER a clean execve hot-reload OR supervisor_reexec_failed fallback=continue_current_binary with per-candidate errno — and in the failure case the pane STAYS ALIVE (no hang) with the restart hint shown. Subset of the broader recyclerestart sync-block work. Cannot be agent-done: needs a live editor + real recycle.
- [ ] [#4myd] [operator-verify] #suprecyclestall live-verify: with the new binary installed (cargo install + cargo build --release done, pushed agent-doc aafa6510), restart a stale session and drive an idle/next-queue-item recycle. Confirm ops.log shows `supervisor_binary_stale_self_recycled ... candidates=[...]` then EITHER a clean execve hot-reload OR `supervisor_reexec_failed fallback=continue_current_binary` with the per-candidate errno — and in the failure case the pane STAYS ALIVE (no hang) with the restart hint shown. Cannot be agent-done: needs a live editor + real recycle.
- [ ] [#recyclerestart] [clean-session] PCP/Supervisor recycle does not restart the agent session (img_21.png); JB Sync Tmux Layout after tmux kill-pane surfaced `Sync deferred: another tmux layout sync is already running`. ROOT CAUSE (code-grounded): the warning fires when agent_doc_sync_try_lock() (ffi.rs:746, a process-wide SYNC_LOCKED AtomicBool shared cross-editor) is already held. SyncLayoutAction.kt holds that FFI lock across the ENTIRE agent-doc sync subprocess (runCommandWithTimeout, line ~299, released only in finally line 331), and that subprocess is bounded only by the 30s SYNC_PROCESS_TIMEOUT_MS and can block on in-pane/binary recovery (the timeout message literally says retry can run binary recovery). So layout GEOMETRY sync is coupled to in-pane readiness, and a second manual sync racing an in-flight one bails immediately (non-blocking compare_exchange) instead of superseding. FIX: decouple tmux geometry sync from in-pane init/recovery; hold SYNC_LOCKED only across the fast geometry reconcile, run pane readiness/recovery separately; make a newer sync generation supersede an in-flight stale one rather than warn-and-skip; prove recycle restarts/clears the harness session. CLEAN session. Plan: tasks/agent-doc/plan-pcp-recycle-no-restart-sync-block.md
- [ ] [#qdedup] [clean-session] Between-queue-turn trigger enqueue must de-duplicate: successive between-turn enqueues concatenated into one garbled trigger (`/clear /agent-doc <FILE>/agent-doc <FILE>`). Operator: stop the drain once a turn completes, insert into the inactive agent between turns, enqueue UNIQUE (one /clear, one /agent-doc <FILE>) landing once in the PCP/Supervisor between-turn stage. Compose next trigger as a deduped set, gate strictly to the inter-turn boundary, proof line between_turn_enqueue deduped=N. Related #qflood2/#qchurn/#clearcontresume. CLEAN session. Plan: tasks/agent-doc/plan-between-turn-trigger-dedup.md
- [ ] [#clearexchstale] [clean-session] JB Clear Exchange clears the exchange but the content comes back (from a stale HEAD/snapshot replay); same session, a deleted+retyped prompt also revived. Same disk-vs-buffer convergence family as #fcc0: a redeliver/replay path re-applies HEAD/snapshot over the cleared buffer, and clear does not advance the durable merge base. FIX: route clear through converge_document_or_disk + advance merge base; gate redeliver on fresh editor-buffer proof (#dvre/#eprf). CLEAN session. Plan: tasks/agent-doc/plan-jb-clear-exchange-stale-head-revival.md
- [ ] [#qpresetstrike] [clean-session] Preset-token queue head (a #name from prompt_presets, e.g. #advance-review) is unstrikeable by any agent path: `queue consume` rejects it as id-backed (leading #), while `--done`/`--pending-gate` reject it as not a backlog id, and `finalize` keeps the head — so it wedges queue_active=true and re-serves every /agent-doc invocation. FIX: classify a #-head against prompt_presets AND backlog/icebox before routing; a preset head should strike like a free-text head via consume + the finalize heuristic, while do[#id] heads stay on the --done path. Workaround: strike ~~#name~~ + drop go + reset --from-current --preserve-session + commit. Relates to #qchurn/#goqueuestall. Plan: tasks/agent-doc/plan-preset-queue-head-unstrikeable.md
- [ ] [#qchurn] [clean-session] go-mode no-op churn: after #goqueuestall tag-defer skips the id-backed [clean-session]/[operator-verify] backlog heads, untagged FREE-TEXT operator bug-report prompts in the ## Queue block still keep queue_active=true and the supervisor idle-watch re-injects queue_trigger=auto each idle boundary -> repeated no-op /agent-doc cycles (no content diff, user_intent_prompt_changes empty, zero drainable work). Halted live 2026-06-13 via admin queue pause (receipt 27, gen 338). FIX: go-mode should treat a queue whose only remaining prompts are non-id free-text reports (no actionable do #id / new user prompt) as not-drainable and stop re-dispatching, same as the undrainable-head skip. Plan: tasks/agent-doc/plan-go-queue-stall.md
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
