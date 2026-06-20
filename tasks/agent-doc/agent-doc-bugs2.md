---
agent_doc_session: b26b9957-8de6-4eae-bb98-5d7fe4d6b781
agent: claude
claude_model: opus
agent_doc_format: template
agent_doc_write: crdt
claude_args: --dangerously-skip-permissions
codex_args: -s danger-full-access
pending_done_guard: warn
agent_doc_lint_dialect: off
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

❯ *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7a9b103bd5b68223dac05a4c449ac03899aba53d6022e82100578b5b0-20260620-040615.md`*

Compacted content:
❯ - Archived 10 response topic(s): Session compacted; Fix the root cause of the corruption; do #qheadstrike / #sqedit-race; 7 more
❯ - Prior summary/context: ❯ *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7a9b103bd5b68223dac...

### Re: do #jbtsiftnosub — opus-4-8

Shipped the cold-start dispatch gate. The bug: JB `Run Agent Doc` auto-started a fresh pane and typed the `agent-doc <FILE>` trigger before the harness TUI was dispatch-ready, so the keystrokes landed in a not-yet-submit-ready composer and the submit never registered.

**Fix (submodule `e279ceef`, 0.34.20, pushed).** The binary already had a "starting actor reroutes are prompt-gated" predicate (promote `starting`→`ready` only after a harness dispatch-ready prompt is observed); the auto-start path was bypassing it and sending as soon as the pane existed. The auto-start dispatch now **waits for the harness dispatch-ready prompt** on the fresh pane (bounded 5s poll using route's existing prompt-shape detection), **re-verifies immediately before the send**, and **fails closed** (ops.log `dispatch_into_starting_pane reason=harness_not_dispatch_ready_before_auto_start_send`) instead of typing into a starting composer — mirroring the existing `#1vhn` dead-shell guard.

**Verification.** New SimWorld test `route_sim_auto_start_dispatch_waits_for_dispatch_ready_prompt_before_send`: an auto-start dispatch into a still-`Starting` pane fails closed (no acceptance, logs the diagnostic); after the dispatch-ready prompt is observed the same dispatch sends and is proven submitted. Full suite **4810 passed**, clippy clean, `cargo build --release` done (refreshes `.bin/agent-doc`).

**Gated `[operator-verify]`** — like the rest of this route family, the live proof is a two-pane tmux repro that can't be agent-run. Unblock: `make install` at a supervisor-recycle boundary (deliberately NOT installed mid-session — it would swap the binary under a live route-owned supervisor and drift finalize), then live `Run Agent Doc` auto-starting a cold pane: confirm the trigger waits for dispatch-ready then submits, and `dispatch_into_starting_pane` appears if a send is attempted early.

### Re: JB Run Agent Doc on monsterrodholders.md duplicates + no submit — opus-4-8

> **Queue prompt:**
>
> do [#jbtsiftnosub]


> **Queue prompt:**
>
> :pushpin: JB `Run Agent Doc` on monsterrodholders.md adds duplicates of the prompt into the input and does not submit ![img_50.png](img_50.png).

Captured as **`#jbdupnosub`**. `img_50.png` shows the `agent-doc /home/brian/work/btakita/agent-loop/src/boost-client/tasks/monsterrodholders.md` trigger typed **five times** as un-submitted draft lines — duplicate injection plus the same no-submit you've been hitting.

This is the **same root** as `#jbtsiftnosub`, seen from the other side: the route attempted the send repeatedly into a pane that was never dispatch-ready, and each retry **re-typed** the trigger instead of submitting — so five copies accumulated and none went through. The cold-start gate I just shipped (0.34.20) addresses the root directly: it **fails closed before re-typing** into a starting/not-ready composer, so the retry-accumulation can't happen. There's one distinct sub-issue worth a separate fix regardless: a not-ready or failed dispatch must **not leave N copies in the composer** — it should clear the input or never type at all.

Like the rest of the family this needs a live repro capture to fully root-cause *why* five retries (cold-start vs dead-shell, what the retry loop saw). **Operator capture at repro:** `.agent-doc/logs/supervisor-stderr.log` + the pane scrollback at send time. Tagged `[operator-verify]`; queued behind the cold-start gate which likely already neutralizes it once 0.34.20 is installed.

### Re: do #avgwire — opus-4-8

> **Queue prompt:**
>
> do [#avgwire]


Wired the auto-verify mechanism where it genuinely applies, and the audit turned out optimistic about the rest. Honest state:

**Wired (1):** `#tsiftmdcrash-verify` now carries `verify=ops_log:capability_proof_live_child_preserved`. That marker is emitted *only* when the shipped fix works (capability-proof gives up but preserves the live harness child), so your live test auto-resolves the gate from ops.log — no manual clearing.

**Why the other audited "already emits a marker" candidates don't cleanly wire yet** — each needs a distinct *success* marker added at its proof site first:

- **`#jbtsiftnosub`** — the success path emits nothing distinctive (it just proceeds to a normal dispatch); only the *fail-closed refusal* logs `dispatch_into_starting_pane`. There's no clean success signal to key auto-resolve on. (I shipped this fix this session; adding a success line on the gate's OK path is the follow-up.)
- **`#agentreloadrestart` / `#restartagentmenu`** — restart-agent Phase 1/2, not shipped: `agent_restart_performed` has **0 emission sites** in the code, so nothing fires in a live test.
- **`#fbwire`** — `record_force_disk_fallback` exists, but its Phase 2 supervisor wiring is the still-gated remaining work, so it won't fire live yet.
- **`#qpausemix-verify`** — its fix prints to `session-check` **stderr**, not `ops.log`, so an `ops_log:` predicate can't observe it.

So exactly one gate qualified today. The "need a marker added first" route/plugin bucket (`#qpauseux`/`#jbrundispatch`/`#1vhn`/`#cyh0`/`#8bfz`/`#qheadstrike`/`#hap7`…) plus the per-proof-site success markers above are all captured as **`#avmark`** — that's the real enabling work to make "test → log → gate auto-closes" cover the whole review list. The mechanism is sound; most gates just don't emit a clean success signal yet.
<!-- agent:boundary:df671de5 -->
<!-- /agent:exchange -->

## Queue

<!-- agent:queue priority preset="#spec-test-build-install-commit-push" priority go -->
- ~~do [#jbtsiftnosub]~~
- ~~:pushpin: JB `Run Agent Doc` on monsterrodholders.md adds duplicates of the prompt into the input and does not submit ![img_50.png](img_50.png).~~
- ~~do [#avgwire]~~
- :pushpin: do [#vgat]
- ~~do [#j9ja]~~
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
- [ ] [#vgat] #jbdupnosub #agent-doc-bug [operator-verify] JB Run Agent Doc on src/boost-client/tasks/monsterrodholders.md typed the agent-doc <FILE> trigger FIVE times (duplicate injection) into the input and did NOT submit any (screenshot tasks/agent-doc/img_50.png: 5 identical un-submitted trigger lines). Same dispatch-no-submit family as #jbtsiftnosub / #jbrundispatch / #1vhn, NEW symptom = duplicate accumulation: the route re-typed the trigger on each send-retry into a not-yet-dispatch-ready pane without clearing or submitting. The #jbtsiftnosub cold-start gate (0.34.20, e279ceef) should PREVENT the accumulation by failing closed before re-typing into a starting composer; the distinct sub-issue is that a not-ready / failed dispatch must NOT leave N copies in the composer (clear the input or never type). Needs operator capture at repro: .agent-doc/logs/supervisor-stderr.log + pane scrollback at send time + whether the pane was cold-starting or a dead shell. Plan: extend tasks/agent-doc/plan-route-dispatch-into-crashed-harness.md (duplicate-retry-injection variant).
<!-- /agent:backlog -->

## Review
 
<!-- agent:review -->
- [/] [#j9ja] avmark [recommended] Add a distinctive SUCCESS ops.log marker at each gated review item proof site so #optverify auto-verify (--pending-set-verify verify=ops_log:<marker>) can resolve them from a live operator test. Per-item work: #jbtsiftnosub log a success line (e.g. auto_start_dispatch_ready_confirmed) on the reverify gate Ok path (today only the fail-closed refusal logs dispatch_into_starting_pane); ship the unshipped restart-agent features then wire their markers (#agentreloadrestart agent_restart_performed has 0 emission sites, #restartagentmenu); wire #fbwire once its Phase 2 supervisor path emits record_force_disk_fallback live; route #qpausemix-verify queue_paused preamble to ops.log or add an ops_log marker (it currently prints to session-check stderr only); add markers for the route/plugin bucket #qpauseux/#jbrundispatch/#1vhn/#cyh0/#8bfz/#qheadstrike/#hap7/#gf5n/#rt83/#docdriftgrace. Only #tsiftmdcrash-verify (capability_proof_live_child_preserved) was cleanly wireable in the #avgwire cycle.
- [/] [#jbtsiftnosub] #agent-doc-bug Operator-reported (2026-06-19): JB Run Agent Doc on tsift.md auto-started a new supervisor/tmux pane, typed the prompt into the Claude input, but did NOT submit it. Dispatch-no-submit on a COLD START — the route typed into a pane that was not yet dispatch-ready (harness still starting), so the submit never landed. Same family as #jbrundispatch (accepted_only dispatch with no submit), #1vhn (typed into a not-ready pane), #runexitrestart (fail-closed instead of restart-then-submit), but the new wrinkle is the cold-start race: route must positively re-verify a harness dispatch-ready prompt AFTER the auto-start pane comes up and immediately BEFORE send, and fail closed (or wait+retry) instead of typing into a starting pane. Capture at repro: .agent-doc/logs/supervisor-stderr.log + route trace + pane state at send time. Overlaps the route/startup invariants in route.rs; needs a live two-pane repro. Plan candidate: extend tasks/agent-doc/plan-route-dispatch-into-crashed-harness.md (starting-pane variant).
- [/] [#editorbufwin] #agent-doc-bug #queue-user-edit-overwrite The IPC content_ours recovery drops unconsumed user queue additions the operator typed into the live editor buffer (operator: "the queue reverted to disk state; the editor buffer should have won as the source of truth"). PLAN DONE: root cause is NOT content_ours composition (it deliberately keeps additions next-cycle-visible on disk per the ignores_unproven_live_queue_deletions invariant — a blind splice-into-content_ours attempt 2026-06-19 tripped that test) but the #pcwc post-commit worktree reconcile overwriting the disk addition with HEAD. GATED for a focused merge-core cycle. Fix: P1 #pcwc reconcile preserves unconsumed user queue additions on disk; P2 auto-recover the commit (automate the manual reset --from-current the operator does today); P4 refresh the stale supervisor CRDT that enables the drift. Plan: tasks/agent-doc/plan-editor-buffer-wins-queue-merge.md
- [/] [#fbwire] [operator-verify] #agent-doc-bug Phase 2 live wiring of the #fullboundary convergence gate (Phase 1 decision core + playback shipped, submodule 17010e78). Call convergence_gate_decision inside the supervisor idle-queue-watch/drain inter-item dispatch (start/idle_watch.rs to start.rs): gather the four facts (committed via cycle_state, editor converged==HEAD via emit_postcommit_worktree_check, ipc_socket inflight==0, actor idle), defer the next trigger until Dispatch, on ForceDiskFallback do the real --force-disk write + record_force_disk_fallback. Add SimWorld acceptance. Live-supervisor-critical: needs make check + make tmux-ci across two live panes, so NOT in-session-loop-drainable; needs a focused operator-driven cycle. Plan: tasks/agent-doc/plan-fullboundary-convergence-gate.md
- [/] [#hap7] [operator-verify] #agent-doc-bug #qdup Operator-reported (2026-06-19): agent-doc duplicated the operator equityfundingsource.md queue prompt (All upload preview dialogs should be full screen, deploy) on a concurrent edit. Second instance of the queue-prompt duplication family (first: virtuousai.md). Fix per operator spec: constrain the CRDT/semantic merge to the structural part it applies to (per-structural-node merges, not whole-component) so a concurrent operator edit to one queue item cannot duplicate adjacent structure; and when the operator deletes a structure that agent content targets, note it in agent:exchange instead of merging. Merge-core change, the corruption source this session, so NOT safe to rush in the in-session loop; needs a focused cycle. Plan: tasks/agent-doc/plan-scoped-crdt-merge-no-structural-duplication.md
- [/] [#qpausemix-verify] [operator-verify] #agent-doc-bug Verify the #qpausemix session-check fix (submodule 2cf7bcee) live: build+install the new binary (make install: cargo install --path . + cargo build --release + agent-doc lib-install) at a supervisor recycle boundary (deferred from this session to avoid finalize-client-newer-than-supervisor drift), then on a controller-paused queue (e.g. equityfundingsource.md under a qflood pause) confirm session-check stderr now prints the "queue_paused is NOT a contradiction" preamble + recorded reason so an agent no longer stalls deciding operator-set-pause vs transient state. Root cause fixed: session-check printed the bare CONTINUATION_NO_STALL_GUIDANCE constant instead of continuation_guidance(pause_reason); resolution previously only reached preflight JSON.
- [/] [#sqedit-race] [operator-verify] #agent-doc-bug Live-IPC-supervisor race on direct queue edits. Phase 1 (#8bfz single live plugin owner per doc) SHIPPED 237f5fb2. Phase 2 (queue-edit lease) SHIPPED d9a0ae58. Phase 3 (idempotent malformed-queue normalization + render(parse(x)) fixed-point property test) SHIPPED 3c4c8bbb (pushed) — normalization via #qdup-bare-id / #qnoise-multiline-strike / #pushpinaccum; fixed-point proof tests render_parse_is_fixed_point_on_malformed_queue + render_parse_fixed_point_property_over_malformed_family, full suite + clippy green. REMAINING: Phase 4 [operator-gated] make install -> agent-doc admin recycle -> queue prune-noise this doc, confirm queue_stale_noise_lines drops to 0 and the prune sticks across a later preflight (no re-injection). Plan: tasks/agent-doc/plan-supervisor-direct-queue-edit-race.md
- [/] [#qheadstrike] [operator-verify] #agent-doc-bug Completed free-text queue heads (no #id) are not struck after a proven closeout. Repro: a single equityfundingsource.md cycle (2026-06-19) answered 4 operator queue heads, reached committed + session-check ok via the reset --from-current -> commit recovery path, yet preflight still showed queue_drainable_head_count=4 / queue_continuation_required=true, so the loop churns and the operator must clear them by hand. Three causes: (1) the finalize strike heuristic only consumes ONE free-text head per cycle; (2) the force-disk / reset->commit recovery paths bypass the strike entirely; (3) queue consume exact-matches and rejects when the live buffer mutated the head (pushpin glyph + item split: "snapshot head prompts do not match document head prompts"). Fix: strike ALL answered free-text heads, move the strike onto the committed seam so recovery paths converge, and normalize head matching. Plan: tasks/agent-doc/plan-freetext-queue-head-autostrike.md
- [/] [#tsiftmdcrash-verify] [operator-verify] Live-verify the #tsiftmdcrash fix (agent-doc submodule a281ae3e): capability-proof give-up no longer SIGTERMs the live hosted harness child. Needs operator install at a supervisor-recycle boundary (make install: cargo install --path . + cargo build --release + agent-doc lib-install — deferred from this session to avoid finalize-client-newer-than-supervisor drift). Then on a managed OpenCode/Codex session force a capability-proof failure (e.g. network blip) and confirm the harness pane stays alive with '<harness>_capability_proof_live_child_preserved reason=dispatch_gated_not_killed' in the session log and NO 'exit_code=143' / 'opencode_exit code=143' kill (the prior crash signature). Root cause was confirmed in tsift-v0.1.log: two opencode_exit code=143 SIGTERMs on pane %78 each at the same instant as opencode_capability_proof status=failed attempts=3. <!-- gate-verify verify="capability_proof_live_child_preserved" set_at="1781930602" -->
- [/] [#runexitrestart] [operator-verify] [recommended] #agent-doc-bug When the harness has cleanly EXITED (supervisor showing "Press Enter to restart, or q to exit") and an operator JB Run Agent Doc / queued prompt-bearing dispatch arrives, the route fails closed (dispatch-only busy -> fail closed) instead of restarting the harness then submitting. route.rs dispatch path has NO clean-exit handling — only session_restart (session_actor_cmd.rs:2235) checks pane_shows_clean_exit_prompt. live_pane_prompt_ready reads the restart screen as not-ready -> AliveBusy, so the dispatch barrier never submits. Fix: the managed route / supervisor idle-queue-watch should detect a clean-exit pane with prompt-bearing work, press Enter to restart the harness, wait for the dispatch-ready prompt, then submit, so a cleanly-exited harness never drops a Run Agent Doc. NOTE: the originally-reported SILENT loss was a stale-supervisor-at-hot-reload artifact already fixed in installed 0.34.18 (idle->ready reconcile now gated on prompt_ready==Some(true) + keyed off a real harness dispatch-ready prompt). Live-supervisor-critical (two-pane tmux verify) -> focused cycle, not in-loop.
- [/] [#jbrundispatch] [operator-verify] #agent-doc-bug JB Run Agent Doc route reliability: (1) after manual pane kill + autostart, Run Agent Doc stalls; (2) some Run Agent Doc ops do not submit to Claude Code (accepted_only dispatch with no submit). Both are route/tmux/startup-miss issues overlapping shipped work (#jbrunstart, startup-miss supersession, #qdispatchloss) and CANNOT be safely fixed blind in the invariant-heavy route.rs. NEEDS a live repro capture at stall time: agent-doc admin detect (cross-doc pane contamination?) + ops.log route trace + which pane state failed, to distinguish a real new bug from already-fixed behavior.
- [/] [#qpauseux] [operator-verify] [recommended] JB Run Agent Doc surfaces a queue_paused/qflood dispatch block to the operator badly, and can appear not to submit at all. Variants: (1) a deliberate/admin pause renders as a raw red Error: toast instead of an actionable notification; (2) a churn-stop pause (reason: broken editor IPC, pausing to stop churn - the #41a32133 exception that blocks every caller including an explicit operator reopen so the route restarts the stale supervisor first) renders as NOTHING - JB Run Agent Doc just silently stalls with no notification; (3) operator report 2026-06-19: JB Run Agent Doc is not submitting / no visible submission result. Plugin should render a paused-queue block (ui_outcome=blocked_with_exact_unblocker unblocker=resume_or_clear_queue_control) as an info/action notification with a Resume button; for the churn-stop/no-submit variant surface a restart-supervisor / resume action instead of silence. Operator action: install the updated JB plugin, then verify both the paused-queue notification and the churn-stop/no-submit notification render (live gate).
- [/] [#docdriftgrace] [operator-verify] agent-doc thrashes when the session doc changes mid/between turn in ways unrelated to the current prompt: snapshot>visible drift (operator trimmed old committed Re: blocks + harness switch + queue edits) made preflight refuse commit (looks like a manual cleanup with stale snapshot) and forced a manual reset --from-current. Binary should classify drift by component and silently auto-rebase snapshot/CRDT for unrelated changes, surfacing ONLY structural conflicts on the in-flight prompt/response. **Operator action: watch one live re-baseline cycle to verify the silent auto-rebase.** Plan: tasks/agent-doc/plan-graceful-doc-drift-resolution.md
- [/] [#gf5n] #numlistrev #agent-doc-bug Operator-reported: numbered/ordered lists rendered REVERSED (4,3,2,1) on an initial turn, boundary after the block. INVESTIGATED 2026-06-19: NOT reproducible in a clean stream/CRDT finalize — a 1-2-3-4 list renders 1-2-3-4 correctly (verified /tmp repro). It manifested only in the drift-heavy monsterrodholders turn, so it is a SYMPTOM of the CRDT merge-corruption family (#hap7/#qdup/#semmerge: the merge inverting list-item order under a live-buffer race), not an independent ordered-list render bug. Resolves when #hap7 scoped-merge lands. Kept open as a #hap7 verification check: confirm numbered-list order survives a merge-during-finalize cycle.
- [/] [#rt83] #qrepin #agent-doc-bug The active-queue stripped diff perpetually reports a phantom pushpin-add of the operator-verify head #qpauseux every preflight even when the doc and snapshot raw queue bodies are byte-identical, keeping no_changes:false and re-triggering the supervisor idle-watch. Investigated: sync_backlog_into_queue (entry_do_id/do_prompt_id), the #mirrorall mirror, and the #7r2s auto-pin (annotate_manual_queue_additions) all correctly recognize and skip the already-pinned bare-id head, so it is NOT a content mutation. Root is in the preflight stripped-diff/boundary-reposition path: commit repositions the boundary in the snapshot but skips the working-tree reposition while a live IPC listener is active, so the post-commit snapshot queue and the post-maintenance doc queue diverge on the pinned operator-verify head position/strip, surfacing a phantom pin add. Cosmetic (no corruption) but it sustains qchurn. Fix: normalize operator-verify mirror pins symmetrically in the stripped diff, or converge the snapshot boundary reposition with the working tree even under a live listener. Needs instrumented per-phase queue-body tracing.
- [/] [#restartagentmenu] restart-agent Phase 2 (after #agentreloadrestart): add a Restart Agent JB plugin menu command — new RestartAgentAction.kt mirroring RestartSupervisorProcessAction.kt, registered in plugin.xml beside AgentDoc.RestartSupervisorProcess, wired through the TerminalUtil restart path; after Phase 1 re-resolves the harness on restart this brings up a changed agent. Decide alongside-vs-replace the existing Restart Supervisor Process action (do not remove without operator confirmation). Bump pluginVersion (gradle.properties, currently 0.2.170), build unsigned + signed zips. LOGGING (operator-install live-gate prove/disprove): log the action invocation + resolved harness. Plan: tasks/agent-doc/plan-restart-agent.md
- [/] [#agentreloadrestart] restart-agent Phase 1 (foundational): make the supervisor restart path re-resolve the harness from current frontmatter so changing agent (claude->opencode) takes effect on restart, and auto-restart the agent at the next idle/turn boundary when the resolved harness differs from the running one. Today the restart loop reuses the harness resolved once at start (SharedSupervisorState.harness, start.rs:171) and never re-reads fm.agent. Re-resolve via HarnessConfig::from_context (harness.rs:116) before respawn, rebuild child_factory/harness_binary, then gate the boundary auto-restart like the supselfheal auto-recycle (prompt_visible and not turn_active, debounced, never mid-turn) behind a new agent_doc_agent_change_restart knob (default ON). LOGGING (live-gate prove/disprove): emit harness_change_detected old/new, agent_restart_boundary_gate reason, agent_restart_performed to ops.log. SimWorld tests + SPEC + VERSIONS. Live-supervisor-critical: focused clean cycle with make check + make tmux-ci. Plan: tasks/agent-doc/plan-restart-agent.md
- [/] [#cyh0] #fcczero #agent-doc-bug [operator-verify] JB File Cache Conflict still occurring; target is ABSOLUTE zero session-doc disk writes except genuine emergencies (everything via editor IPC). Audit (#smconv cycle) confirms the documented hot-path sites are already listener-guarded: post-commit worktree reconcile skips disk with a listener (#pcwcdiskfree), boundary reposition queues via file IPC and only disk-writes when IPC is Unavailable, queue/pending/status maintenance route through converge_or_disk_write (#fccqueue/#fccaudit); remaining disk writes are recovery/scaffold/migration emergencies by design. So the remaining FCC has an UNIDENTIFIED trigger. **Operator action: capture which exact action raises the dialog (the agent-doc command + ops.log line at that moment, e.g. write_authority action=... transport=disk) so the specific unguarded path can be found.** Likely candidates to check: the ack-content sidecar write, the VCS-refresh/save-document signal, or an editor-IPC convergence that the plugin still surfaces as a conflict. Note: this session most FCC came from manual `git checkout HEAD` recovery (a disk write), which #smconv removes the need for by merging drift instead of dropping responses.
- [/] [#8bfz] #fcconeowner #agent-doc-bug elect a SINGLE live IntelliJ plugin consumer per document so concurrent windows/instances cannot race patch application + saveDocument into File Cache Conflicts. Today each IntelliJ plugin process generates its own consumer id jetbrains-<pid>-<uuid> (TypingTracker.kt) and every live instance that has the project open watches .agent-doc/patches/ and consumes patches independently = N live consumers for one doc when N windows are open. #fccreap (0.34.8) reaps DEAD-pid consumer files but does not dedupe LIVE ones. Add single-owner election: the plugin acquires a per-document owner lease (lock file under .agent-doc/, or via the sessions.json registry) and non-owner instances defer patch application + saveDocument for that doc, so exactly one live consumer applies/saves. Owner lease is short-TTL/self-healing so closing the owner window hands ownership to another live instance. Plugin-side change (PatchWatcher + a lease) + binary lease primitive; operator-verify (needs multi-window live test).
- [/] [#1vhn] #routedeadshell agent-doc-bug: JB Run Agent Doc dispatched the agent-doc FILE trigger into a bare zsh % shell on recruit/virtuousai.md (harness had crashed/exited to a shell), so it did not submit (screenshot img_48.png, ~14:03, during a /model opus switch). REPRO (operator 2026-06-18): Run Agent Doc on virtuousai.md -> supervisor + tmux pane CRASHED 2 times -> 3rd attempt stalled -> operator manually ran agent-doc. So the supervisor/pane crash on this dispatch path is REPRODUCIBLE, not a one-off. Route must positively identify a HARNESS dispatch-ready prompt and treat a bare shell prompt as NOT-ready -> fail closed with claim/restart guidance (or auto-restart) instead of typing into the shell; re-verify the harness prompt immediately before send to close the crash-mid-dispatch race; add a dead-harness route diagnostic + SimWorld coverage. Separately root-cause WHY the supervisor+pane crash repeats (the /model opus switch/restart is the lead). **Operator action: if it recurs, capture .agent-doc/logs/supervisor-stderr.log + the pane scrollback at the crash moment.** Plan: tasks/agent-doc/plan-route-dispatch-into-crashed-harness.md
<!-- /agent:review -->

<!-- agent:icebox -->
<!-- /agent:icebox -->

## Completed / Reaped

<!-- agent:done archive=tasks/agent-doc-bugs2.done.md -->

<!-- completed work archived in tasks/agent-doc-bugs2.done.md -->
<!-- /agent:done -->
