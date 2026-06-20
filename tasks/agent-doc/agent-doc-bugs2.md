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

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7a9b103bd5b68223dac05a4c449ac03899aba53d6022e82100578b5b0-20260619-234312.md`*

Compacted content:
- Archived 7 response topic(s): Fix the root cause of already-done / noise heads flooding the queue; agent-doc duplicating queue items; JB `Run Agent Doc` route stalls (pane-kill+restart / "doesn't submit"); 4 more
- Prior summary/context: prior compacted content: Archived 4 response topic(s): Fix equityfundingsource.md "Mixed signals"; Clean up the noise...

### Re: #sqedit-race (queue-edit race) — opus-4-8

> **Queue prompt:**
>
> :pushpin: [#sqedit-race]


You pinned `#sqedit-race` to the top of a freshly-cleared session — the focused clean cycle this safety-critical commit-path item asks for. I scoped it against the actual code first, and the headline is that it was further along than the plan/backlog showed:

**Phase 1 (#8bfz — single live plugin owner per doc) was already SHIPPED** (commit `237f5fb2`): `plugin_owner.rs` lease (`.agent-doc/plugin-owner/<hash>.json`, 30s TTL) + FFI (`agent_doc_plugin_owner_try_acquire`/`_release`) + `PatchWatcher.ownsDocument()` gate + release-on-dispose + 5 unit tests. Non-owner IntelliJ instances already defer patch application + saveDocument, so the multi-consumer re-flush source is closed. The plan now records this.

**Phase 2 (queue-edit lease) — implemented + committed this cycle** (`d9a0ae58`, superproject `9ec17cc81`):
- New `queue_edit_owner` module — a per-document `.agent-doc/queue-edit-owner/<hash>.json` lease (~15s self-healing TTL, mirrors the `drain-owner` lease) held for the whole `queue prune-noise` / `queue consume` read-modify-write via an RAII `QueueEditGuard` (released on drop, including early return / error).
- The two *other* concurrent queue writers now defer while a **different, live** process holds a fresh lease: `run_queue_maintenance` returns early without mutating (logs `queue_maintenance_deferred reason=queue_edit_lease`), and the supervisor idle-queue-watch skips the dispatch tick (logs `idle_queue_watch_drain_skipped reason=queue_edit_in_flight`). The short TTL makes this a brief yield, not a stall.
- This closes the preflight half of source (2) and source (3) from the plan: preflight maintenance / idle-watch can no longer round-trip a torn intermediate queue mid-edit.
- 5 lease unit tests + `run_queue_maintenance_defers_while_foreign_queue_edit_lease_held` (proves zero mutation under a foreign lease, then resumes once cleared). FlowCore budget bumped (`maintenance.rs` `reason=` 3→4). **Full suite 4796 passed + clippy clean.**

**Remaining:**
- **Phase 3** (idempotent normalization) is *mostly landed* already — `#qdup-bare-id`, `#qnoise-multiline-strike`, `#pushpinaccum`. What's left is a `render(parse(x))` fixed-point property/SimWorld test over an already-malformed queue (double-pins + stray `~~~done`/`---` runs together) to prove convergence vs amplification.
- **Phase 4** is the operator-gated finish and the only live-verify step: `make install` → `agent-doc admin recycle` → `queue prune-noise` on this doc, confirming `queue_stale_noise_lines` → 0 and the prune sticks across a later preflight.

**Install deliberately deferred:** this session + the running supervisor (pid 3024307) both use `~/.cargo/bin/agent-doc`, so a mid-session `cargo install` would make the finalize client newer than the supervisor → `live_prompt_drift`. The new binary ships at the Phase 4 `make install` + recycle boundary. CI for `d9a0ae58` is in progress; local evidence is green.

No agent-drainable queue heads remain (all `[operator-verify]`), so I'm not auto-looping.

### Re: Fix the root cause of the degraded session — opus-4-8

Found and fixed the actual root cause (not the stale binary I first suspected — that was a secondary factor).

**Root cause: the realtime cross-editor broadcast was storming patches to dead editors.** `realtime_model::broadcast_editor_change` built its peer set from *every* `.agent-doc/live-buffer` sidecar with **no liveness check**. This document had accumulated **187 dead-`jetbrains-<pid>` orphan sidecars** (one per closed IntelliJ window, over days). So a single live editor (pid 3873180) reporting a buffer change fanned out to all 187 dead peers — I caught **247 `realtime_broadcast_queued` events in a few minutes**. Each broadcast (a) re-created the dead-pid patch file the `#fccreap` reaper had just cleared, and (b) merged against that dead editor's *divergent stale buffer* (merged_len 6300…73989). One of those stale merged buffers (19278 bytes) is what surfaced in my last finalize as the `socket_ack_content` drift candidate → `live_prompt_drift_after_preflight` + the post-commit working-tree reconcile, on *every* finalize.

**Code fix (durable, shipped — agent-doc `5397b89f`):** `broadcast_editor_change` now liveness-filters both ends:
- a dead-pid **originator** skips the whole broadcast (`reason=dead_origin_editor`);
- dead-pid **peers** are dropped *and* their orphan live-buffer sidecars reaped (`realtime_broadcast_dead_peers_reaped`).

JetBrains ids carry the owning pid; non-JetBrains ids (no embedded pid) are conservatively treated as live. 3 new tests; full suite **4799 + clippy green**.

**Live session fixed (operational):** I removed the 187 dead-jetbrains orphan sidecars for this doc (leaving the 4 live-editor + 19 legacy no-id sidecars untouched, and the concurrent equityfundingsource session untouched). The storm stopped immediately — **0 broadcasts since** — because with the dead peers gone there are no stale targets to fan out to, regardless of the running binary version.

**Secondary factor (noted, not the cause):** the swapped binary timing — `~/.cargo/bin/agent-doc` was reinstalled at 20:01 (concurrent EFS session/build) while this session's supervisor (pid 3024307) keeps running its 17:20 image — exercised `#supselfheal`. I deliberately did **not** `cargo install` (it would re-stale the concurrent EFS supervisor too); the supervisor self-recycles at its next idle boundary, and the broadcast fix itself ships at the next proper install boundary.

**Follow-up queued** (`#lbreap`, recommended): extend `#fccreap` to proactively reap dead-pid live-buffer sidecars at closeout, so orphans never accumulate to 187 between broadcasts in the first place.

### Re: #lbreap done + "deleted queue items came back after a File Cache Conflict" — opus-4-8

**`#lbreap` shipped** (agent-doc `c5b5cc35`): closeout now reaps dead-PID live-buffer sidecars (`reap_stale_jetbrains_live_buffers`, beside the existing `#fccreap` patch-file reaper), so closed-IntelliJ orphans self-clear every cycle instead of accumulating to the 187 that fueled the storm. 3 tests; full suite **4802 + clippy green**. Marked done.

**On your queued observation** ("I deleted struck queue items in monsterrodholders.md, then got a JB File Cache Conflict and they came back — the editor buffer should be the source of truth"): that is the **same root cause** I just fixed this session. The deleted items came back because the realtime broadcast was re-injecting a *stale/dead editor's* buffer (which still contained the struck items) back over your edit — and the File Cache Conflict was the stale buffer fighting the on-disk truth. Concretely:
- A closed/stale IntelliJ left an orphan live-buffer sidecar holding the pre-deletion queue.
- The broadcast fanned that stale buffer out as a "peer" with no liveness check, re-materializing the struck items + raising the FCC.
- `#sqdrift` (liveness-filter the broadcast origin + peers, reap dead peers) + `#lbreap` (proactively reap dead sidecars at closeout) stop both: a dead/stale editor can no longer originate or receive a broadcast, so it can't resurrect deleted items.

Your principle — *the live editor buffer is the source of truth* — is preserved: the fix removes only **dead** editors from the picture; a single **live** editor's buffer still wins. (The remaining residual this session is a separate, pre-existing class — a *live* IntelliJ, pid 3873180, holding a stale buffer for this doc; each finalize detects that drift and commits the correct `content_ours` to HEAD, so the response is never lost, just noisier. Tracked under the `#cyh0`/`#fccaudit` stale-live-editor family, distinct from the storm.)

Net: the degraded session's root cause (broadcast storming dead editors → regenerated patch files + stale-buffer drift on every finalize) is fixed in code and the live storm is stopped (0 broadcasts since cleanup). Remaining queue heads are all `[operator-verify]`.
<!-- agent:boundary:589efd78 -->
<!-- /agent:exchange -->

## Queue

<!-- agent:queue priority preset="#spec-test-build-install-commit-push" priority go -->
- :pushpin: For all `[operator-verify]` issues, look in the logs for verification.
- :pushpin: do [#tsiftmdcrash]: Look in the logs for the crash.
- :pushpin: [#sqedit-race]
- :pushpin: [#qheadstrike]
- :pushpin: do [#hap7]
- :pushpin: do [#fbwire]
- :pushpin: [#qpausemix-verify]
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
- [ ] [#qheadstrike] [operator-verify] #agent-doc-bug Completed free-text queue heads (no #id) are not struck after a proven closeout. Repro: a single equityfundingsource.md cycle (2026-06-19) answered 4 operator queue heads, reached committed + session-check ok via the reset --from-current -> commit recovery path, yet preflight still showed queue_drainable_head_count=4 / queue_continuation_required=true, so the loop churns and the operator must clear them by hand. Three causes: (1) the finalize strike heuristic only consumes ONE free-text head per cycle; (2) the force-disk / reset->commit recovery paths bypass the strike entirely; (3) queue consume exact-matches and rejects when the live buffer mutated the head (pushpin glyph + item split: "snapshot head prompts do not match document head prompts"). Fix: strike ALL answered free-text heads, move the strike onto the committed seam so recovery paths converge, and normalize head matching. Plan: tasks/agent-doc/plan-freetext-queue-head-autostrike.md
- [ ] [#sqedit-race] [operator-verify] #agent-doc-bug Live-IPC-supervisor race on direct queue edits. Phase 1 (#8bfz single live plugin owner per doc) SHIPPED 237f5fb2. Phase 2 (queue-edit lease: prune-noise/consume hold a queue_edit_owner lease that preflight queue maintenance + supervisor idle-watch defer to while a different live process holds it) SHIPPED d9a0ae58 — full suite 4796 + clippy green. REMAINING: Phase 3 add a render(parse(x)) fixed-point property/SimWorld test over an already-malformed queue (normalization itself mostly landed via #qdup-bare-id / #qnoise-multiline-strike / #pushpinaccum); Phase 4 [operator-gated] make install -> agent-doc admin recycle -> queue prune-noise this doc, confirm queue_stale_noise_lines drops to 0 and the prune sticks across a later preflight (no re-injection). Plan: tasks/agent-doc/plan-supervisor-direct-queue-edit-race.md
- [ ] [#qpausemix-verify] [operator-verify] #agent-doc-bug Verify the #qpausemix session-check fix (submodule 2cf7bcee) live: build+install the new binary (make install: cargo install --path . + cargo build --release + agent-doc lib-install) at a supervisor recycle boundary (deferred from this session to avoid finalize-client-newer-than-supervisor drift), then on a controller-paused queue (e.g. equityfundingsource.md under a qflood pause) confirm session-check stderr now prints the "queue_paused is NOT a contradiction" preamble + recorded reason so an agent no longer stalls deciding operator-set-pause vs transient state. Root cause fixed: session-check printed the bare CONTINUATION_NO_STALL_GUIDANCE constant instead of continuation_guidance(pause_reason); resolution previously only reached preflight JSON.
- [ ] [#hap7] [operator-verify] #agent-doc-bug #qdup Operator-reported (2026-06-19): agent-doc duplicated the operator equityfundingsource.md queue prompt (All upload preview dialogs should be full screen, deploy) on a concurrent edit. Second instance of the queue-prompt duplication family (first: virtuousai.md). Fix per operator spec: constrain the CRDT/semantic merge to the structural part it applies to (per-structural-node merges, not whole-component) so a concurrent operator edit to one queue item cannot duplicate adjacent structure; and when the operator deletes a structure that agent content targets, note it in agent:exchange instead of merging. Merge-core change, the corruption source this session, so NOT safe to rush in the in-session loop; needs a focused cycle. Plan: tasks/agent-doc/plan-scoped-crdt-merge-no-structural-duplication.md
- [ ] [#fbwire] [operator-verify] #agent-doc-bug Phase 2 live wiring of the #fullboundary convergence gate (Phase 1 decision core + playback shipped, submodule 17010e78). Call convergence_gate_decision inside the supervisor idle-queue-watch/drain inter-item dispatch (start/idle_watch.rs to start.rs): gather the four facts (committed via cycle_state, editor converged==HEAD via emit_postcommit_worktree_check, ipc_socket inflight==0, actor idle), defer the next trigger until Dispatch, on ForceDiskFallback do the real --force-disk write + record_force_disk_fallback. Add SimWorld acceptance. Live-supervisor-critical: needs make check + make tmux-ci across two live panes, so NOT in-session-loop-drainable; needs a focused operator-driven cycle. Plan: tasks/agent-doc/plan-fullboundary-convergence-gate.md
- [ ] [#tsiftmdcrash] [operator-verify] Operator reported that a tsift.md agent-doc turn crashed and killed the session process while the tmux pane stayed active. Supervisor pid 1742954 is alive; pane %78 OpenCode TUI shows Thinking normally; tsift-v0.1.log (994KB) shows NO panic/abort/crash signatures — only two intentional supervisor_binary_stale_self_recycled via=execve_preserve_child entries from cargo install + make install (#supkill-bg blue/green drain-and-supersede). Most plausible: operator saw the brief execve-recycle window (current_exe deleted) as a crash. Not reproducible from logs alone. **Operator action: paste the exact crash error text / pane capture from the crash moment, or the specific cycle id, so it can be root-caused.**
<!-- /agent:backlog -->

## Review
 
<!-- agent:review -->
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
