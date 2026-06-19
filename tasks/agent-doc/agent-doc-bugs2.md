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

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7a9b103bd5b68223dac05a4c449ac03899aba53d6022e82100578b5b0-20260619-213455.md`*

Compacted content:
- Archived 4 response topic(s): Fix equityfundingsource.md "Mixed signals"; Clean up the noise queue heads; Fix live IPC supervisor races on direct queue edits + clean noise after restart; 1 more
- Prior summary/context: prior compacted content: Archived 5 response topic(s): queue drain state; JB `Run Agent Doc` is not submitting; #docd...

### Re: Fix the root cause of already-done / noise heads flooding the queue — opus-4-8

Found and fixed the real root cause. It was never about a stale IDEA buffer — it was that `queue prune-noise` **couldn't see most of the noise**, so nothing could clear it on disk and the convergence kept re-pushing it to the editor.

**Root cause.** The queue has two parsers that disagreed about what a "head" is:
- `queue::parse` (what preflight surfaces) recognizes `:round_pushpin:`/`---`-fenced blocks and bare ```` ``` ```` console pastes.
- `item_nodes` (the *strike* path that `prune-noise`/`consume`/dedup use) recognizes **only bulleted `- …` lines and skips fenced code**.

So every operator-pasted console dump — the bulk of the flood — was invisible to the strike path. `prune-noise` reported "nothing to prune" while 18 KB of pasted blocks sat on disk forever. A `#5eq8` stray id inside a console dump even made it look like a *drainable* head.

**The fix (`#qnoise-multiline-strike`, agent-doc 0.34.15):**
1. New `queue::parse_spans` — one byte-range-aware source of head segmentation. `prune-noise` now excises multiline `---`-fenced noise `Prompt` blocks **and** bare-```` ``` ````-fenced `Freeform` console pastes by exact range, alongside the existing bulleted strike. `is_noise_freeform_line` preserves real `---`/`~~~` separators and `re [#id]` references.
2. The drainability classifier now demotes any **multi-line** head text to noise (kills the `#5eq8`-in-a-dump false positive) — but a single-line `do [#id]` that merely happens to be `---`-wrapped stays drainable, so no real directive is lost. Drain, the `queue_stale_noise_lines` counter, and `prune-noise` now agree.

**Verified on the actual flooded doc (on a copy):** `prune-noise` excised **61** noise entries — 0 ``` fences / `:round_pushpin:` / `agent:boundary` left — while preserving every id-backed directive, including the `---`-wrapped `#tsiftmdcrash`. New regression test + full queue suite (479) + `make check` green.

This directly unblocks `#prunenoise-live` and the "noise heads never auto-deleted" half of `#sqedit-race`. Next: I'll install 0.34.15 + recycle the supervisor, then run `queue prune-noise` on this live doc to clear the flood with matched binaries.

### Re: agent-doc duplicating queue items — opus-4-8

> **Queue prompt:**
>
> :pushpin: In equityfundingsource.md, I typed in queue items and agent-doc duplicated the queue items.


Reproduced and fixed (`#qdup-bare-id`, 0.34.16). Your report is corroborated right here — this doc's live queue had `[#sqedit-race]` and `[#qpausemix-verify]` each appearing **twice**.

**Root cause.** The only id-dedup wired into preflight maintenance was the AST **node-key** dedup, which deliberately preserves occurrence-indexed duplicates. The id-aware `dedup_live_prompts` was never wired in — on purpose: a duplicate `do [#id]` **directive** can be intentional "run it twice" intent (the `#queue-dedup-destroys-intentional-duplicates` invariant). But the backlog→queue mirror / CRDT replay re-emits a **bare** `[#id]` reference head (no `do`), and *nothing* collapsed those — so every mirror pass stacked another copy.

**The fix.** New `queue::dedup_bare_id_reference_heads` runs in `run_queue_maintenance` after the node-key dedup. It collapses duplicate **bare** `[#id]` / `#id` reference heads (pin markers stripped) to the first occurrence, while:
- leaving `do [#id]` **directive** duplicates intact (intentional intent — invariant preserved), and
- never touching free-text heads (including a directive that merely *cites* an id with trailing text, e.g. `#id continue the drain`) or multiline blocks.

**Verified:** new dedup tests + the intentional-`do`-duplicate invariant + full queue (481) and maintenance (67) suites + `make check` all green. Once 0.34.16 is live (installing + recycling next), the next preflight auto-collapses the duplicate heads on this doc.

This is one facet of the broader `#sqedit-race` queue-edit-race item.

### Re: JB `Run Agent Doc` route stalls (pane-kill+restart / "doesn't submit") — opus-4-8

> **Queue prompt:**
>
> :pushpin: Some JB `Run Agent Doc` operations do not submit to Claude Code.


> **Queue prompt:**
>
> :pushpin: In equityfundingsource.md, I killed the pane and autostarted a new pane to resolve a wedge issue. After restarting, JB `Run Agent Doc` stalled.


Investigated both JB-`Run Agent Doc` reports. These are route/tmux dispatch issues, and the honest call is that **they can't be safely fixed blind** — `route.rs` carries dozens of shipped invariants (fresh-pane authority, stale startup-miss supersession, late associated-pane proof, dispatch-only accepted-vs-submitted, starting-actor reroute gating), and a guess-edit risks regressing them. Both reports also overlap heavily with already-shipped work (`#jbrunstart` starting-actor refusal, the startup-miss supersession rules, the `#qdispatchloss` uncommitted-head guard).

**Likely mechanisms (from the code, not a confirmed repro):**
- *"Killed the pane + autostarted a new pane → Run Agent Doc stalled."* After a manual pane kill, the registry/session-log can still name the dead pane as owner; route then either fails closed on late associated-pane proof or waits on a `starting` actor that never reaches a dispatch-ready prompt. The fresh-pane-authority + startup-miss-supersession rules are supposed to cover this, so a stall here is either a gap in superseding the dead pane's `startup_miss` provenance or a readiness-timeout.
- *"Some Run Agent Doc ops don't submit to Claude Code."* Claude Code has no prompt-submit hook, so dispatch-only is `accepted_only` (keys delivered to the pane) without submit proof. If the pane wasn't truly dispatch-ready when keys landed, the text sits in the composer unsent — accepted but not submitted.

**Why I'm not blind-fixing:** without a live repro + `agent-doc admin detect` (cross-doc pane contamination?) + the ops.log route diagnostics from the failing moment, I can't tell a real new bug from already-fixed behavior, and editing the invariant-heavy route path on a guess is how regressions ship. Capturing this as an operator-verify item that needs one captured repro.

Filed `#jbrundispatch` (operator-verify). The fix needs: the failing `admin detect` + ops.log/route trace at stall time.
<!-- agent:boundary:e15e41e8 -->
<!-- /agent:exchange -->

## Queue

<!-- agent:queue priority preset="#spec-test-build-install-commit-push" priority go -->
- ~~:pushpin: In equityfundingsource.md, I killed the pane and autostarted a new pane to resolve a wedge issue. After restarting, JB `Run Agent Doc` stalled.~~
- ~~:pushpin: Some JB `Run Agent Doc` operations do not submit to Claude Code.~~
- :pushpin: In equityfundingsource.md, I typed in queue items and agent-doc duplicated the queue items.
- :pushpin: In equityfundingsource.md, I killed the pane and autostarted a new pane to resolve a wedge issue. After restarting, JB `Run Agent Doc` stalled.
- :pushpin: [#sqedit-race]
---
:pushpin: do [#tsiftmdcrash]
---
- :pushpin: [#qpausemix-verify]
- :pushpin: do [#fbwire]
- :pushpin: do [#hap7]
- :pushpin: Some JB `Run Agent Doc` operations do not submit to Claude Code.
- ~~[#prunenoise-live]~~
- :pushpin: In equityfundingsource.md, I typed in queue items and agent-doc duplicated the queue items.
- :pushpin: In equityfundingsource.md, I killed the pane and autostarted a new pane to resolve a wedge issue. After restarting, JB `Run Agent Doc` stalled.
- :pushpin: Some JB `Run Agent Doc` operations do not submit to Claude Code.
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
- [ ] [#jbrundispatch] [operator-verify] #agent-doc-bug JB Run Agent Doc route reliability: (1) after manual pane kill + autostart, Run Agent Doc stalls; (2) some Run Agent Doc ops do not submit to Claude Code (accepted_only dispatch with no submit). Both are route/tmux/startup-miss issues overlapping shipped work (#jbrunstart, startup-miss supersession, #qdispatchloss) and CANNOT be safely fixed blind in the invariant-heavy route.rs. NEEDS a live repro capture at stall time: agent-doc admin detect (cross-doc pane contamination?) + ops.log route trace + which pane state failed, to distinguish a real new bug from already-fixed behavior.
- [ ] [#sqedit-race] [operator-verify] #agent-doc-bug Fix the live-IPC-supervisor race on direct queue edits (the "noise heads never auto-deleted - the live IPC supervisor races on direct queue edits" hazard). NOT a single converge-path bug (try_editor_converge is already fail-closed on ack_mismatch); it is the compounding of: (1) multiple live JB plugin consumers re-flushing stale buffers (#8bfz), (2) preflight queue maintenance (#mirrorall mirror + backlog->queue sync + #7r2s pin + live-buffer converge) round-tripping already-malformed entries (double-pins, stray ~~~done / --- runs) and re-mangling them, (3) supervisor idle-watch reading a torn queue. SAFETY-CRITICAL supervisor/commit-path - focused clean cycle with make check + make tmux-ci, NOT a degraded-session drain (same class as the deferred #aqhr/#smconv). Phases: 1 single live plugin owner per doc (absorbs #8bfz), 2 queue-edit lease respected by supervisor idle-watch AND preflight maintenance, 3 idempotent malformed-entry normalization (render/parse fixed point), 4 restart + queue prune-noise cleanup (folds in #prunenoise-live + #qpausemix-verify). Plan: tasks/agent-doc/plan-supervisor-direct-queue-edit-race.md
- [ ] [#qpausemix-verify] [operator-verify] #agent-doc-bug Verify the #qpausemix session-check fix (submodule 2cf7bcee) live: build+install the new binary (make install: cargo install --path . + cargo build --release + agent-doc lib-install) at a supervisor recycle boundary (deferred from this session to avoid finalize-client-newer-than-supervisor drift), then on a controller-paused queue (e.g. equityfundingsource.md under a qflood pause) confirm session-check stderr now prints the "queue_paused is NOT a contradiction" preamble + recorded reason so an agent no longer stalls deciding operator-set-pause vs transient state. Root cause fixed: session-check printed the bare CONTINUATION_NO_STALL_GUIDANCE constant instead of continuation_guidance(pause_reason); resolution previously only reached preflight JSON.
- [ ] [#hap7] [operator-verify] #agent-doc-bug #qdup Operator-reported (2026-06-19): agent-doc duplicated the operator equityfundingsource.md queue prompt (All upload preview dialogs should be full screen, deploy) on a concurrent edit. Second instance of the queue-prompt duplication family (first: virtuousai.md). Fix per operator spec: constrain the CRDT/semantic merge to the structural part it applies to (per-structural-node merges, not whole-component) so a concurrent operator edit to one queue item cannot duplicate adjacent structure; and when the operator deletes a structure that agent content targets, note it in agent:exchange instead of merging. Merge-core change, the corruption source this session, so NOT safe to rush in the in-session loop; needs a focused cycle. Plan: tasks/agent-doc/plan-scoped-crdt-merge-no-structural-duplication.md
- [ ] [#fbwire] [operator-verify] #agent-doc-bug Phase 2 live wiring of the #fullboundary convergence gate (Phase 1 decision core + playback shipped, submodule 17010e78). Call convergence_gate_decision inside the supervisor idle-queue-watch/drain inter-item dispatch (start/idle_watch.rs to start.rs): gather the four facts (committed via cycle_state, editor converged==HEAD via emit_postcommit_worktree_check, ipc_socket inflight==0, actor idle), defer the next trigger until Dispatch, on ForceDiskFallback do the real --force-disk write + record_force_disk_fallback. Add SimWorld acceptance. Live-supervisor-critical: needs make check + make tmux-ci across two live panes, so NOT in-session-loop-drainable; needs a focused operator-driven cycle. Plan: tasks/agent-doc/plan-fullboundary-convergence-gate.md
- [ ] [#tsiftmdcrash] [operator-verify] Operator reported that a tsift.md agent-doc turn crashed and killed the session process while the tmux pane stayed active. Supervisor pid 1742954 is alive; pane %78 OpenCode TUI shows Thinking normally; tsift-v0.1.log (994KB) shows NO panic/abort/crash signatures — only two intentional supervisor_binary_stale_self_recycled via=execve_preserve_child entries from cargo install + make install (#supkill-bg blue/green drain-and-supersede). Most plausible: operator saw the brief execve-recycle window (current_exe deleted) as a crash. Not reproducible from logs alone. **Operator action: paste the exact crash error text / pane capture from the crash moment, or the specific cycle id, so it can be root-caused.**
<!-- /agent:backlog -->

## Review
 
<!-- agent:review -->
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
