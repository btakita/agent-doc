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
  '#advance-review': Go through the review items. Add actionable backlog + queue items to complete or advance the review items.,
  '#agent-doc-bug': Please create a plan for agent-doc to fix this issue. Add to the backlog of tasks/agent-doc/agent-doc-bugs2.md
  '#code-review': Please review agent-doc codebase. Inspect the architecture and look for edge cases. Isolate concurrency issues and other workflow bugs. Let's find root causes to fix.
  '#commit-push': commit + push all uncommitted files
  '#follow-up-backlog': Any follow-up items to place in the backlog?
  '#mermaid': Include explanatory mermaid diagram(s).
  '#next-steps': Any follow-up items to place in the backlog? Please ensure the backlog is in priority order. Print the item id of the top of the backlog.
  '#simworld': Strengthen Simworld tests.
  '#spec-test-build-install-commit-push': red-green-refactor. update spec + tests. build + install for local testing. commit + push
  '#spec-test-commit-push': red-green-refactor. update spec + tests. commit + push
queue: start
---

## Status

<!-- agent:status patch=replace -->

done
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7a9b103bd5b68223dac05a4c449ac03899aba53d6022e82100578b5b0-20260718-144129.md`*

Compacted content:
- Archived 3 response topic(s): cycle with no new prompt; repeated no-op invocations were a supervisor resubmit flood; stale verification-item sweep and the queue auto-strike fix
- Prior summary/context: prior compacted content: Archived 2 response topic(s): JB harness switch deferred against a stale actor record; agent...
❯ Can we speed up compact exchange? #spec-test-build-install-commit-push

❯ JB `Run Agent Doc` on equityfundingsource.md + this document has an error: `route deferred for /home/brian/work/btakita/agent-loop/tasks/professional/equityfundingsource.md: Lazily current transition remained delivery_pending for 5000ms; retry after it settles`

### Re: Can we speed up compact exchange? — opus-4-8

Yes. Found a fixed ~2s stall on the hot path, fixed it, shipped as JetBrains plugin 0.2.278.

**Measurement first.** From `ops.log`, compact `wait_ms` sat at 2646–8695ms with a ~4.5s floor that did *not* scale with payload — 46KB→6007ms, 130KB→2646ms. Fixed overhead, not encoding or document size, so tuning the CRDT payload was the wrong target.

**Root cause.** The plugin's `requestRemoteDrain` gated *controller-published* CRDT remote events behind the speculative no-op drain backoff (`shouldStartRemoteDrainUtil`). That backoff exists to stop a self-driven drain spin when there is nothing to pull, and it doubles from 100ms toward a 30s ceiling on an idle document. I confirmed the escalation in the measured pull cadence: 1s, 1s, 2s, 4s, 7s, 13s, 27s, 30s.

An idle document is exactly the state you're in when you hit Compact Exchange. So the published frontier got suppressed and nothing moved until the binary escalated to `ack_recovery_force_refresh` at `CRDT_ACK_FORCE_REFRESH_AFTER_MS` (2000ms) — the only path that called the backoff-bypassing `requestUrgentRemoteDrain`. That 2s toll was paid on **every** compact and finalize against a live editor, not just compact.

**Fix.** A controller push is positive evidence the CPC already holds a frontier, so it now drains urgently instead of being suppressed. `request_full_state` stays exempt (it owns the text-adopt path). This cannot reintroduce the spin the backoff was added for: controller pushes are externally rate-limited to one per ACK-replay interval and only while a write awaits ACK, and the urgent path still falls back to the *gated* drain when it finds no work — so speculative polling stays governed.

Second, compounding half: `requestUrgentRemoteDrain` never reset `consecutiveNoOpReschedules`, so even after applying useful work the gate stayed parked at its previous (up to 30s) delay and re-suppressed the *next* push. Now reset on useful work.

The binary-side 2s escalation is retained as a backstop for older plugin builds — no Rust change needed.

**Shipped.** 3 new regression tests (all green), `make check` green (exit 0), spec updated in `specs/14-realtime-workflow.md`, plugin built + signed + installed to all 4 IDE roots, committed and pushed as `5b648f338`. CI run 29650379566 is pending — recorded, not blocking.

One thing I cannot verify from here: the Kotlin plugin change needs an IDE restart to load (`reload-lib` only swaps the cdylib). Queued as `#crdtpushdrainverify` so you can eyeball the actual compact latency after restart — I expect the ~4.5s floor to drop to roughly the pull/apply round trip.

### Re: JB `Run Agent Doc` route deferred — delivery_pending 5000ms — opus-4-8

Same family as the compact-exchange stall I just fixed, with a residual gap that fix didn't close. Fixed and pushed as `6a2261b96`.

**What happened.** `await_idle_with_max_wait_and_effects` (`agent-doc-route-io/src/startup_debounce.rs`) waits for `delivery_converged: true`. On seeing `delivery_pending` it sent **one** urgent CRDT drain request, latched behind a one-shot `urgent_drain_requested` flag, then polled every 100ms until `max_wait` (5000ms) and bailed.

The problem is that a single urgent drain can legitimately apply nothing: `drainRemoteUpdatesFor` returns early while the path has pending local edits or is mid editor-apply, and the forwarder may not be registered yet. Its only follow-up is the *gated* `requestRemoteDrain` — which is exactly the call an idle document's escalated no-op backoff suppresses. So route spent its remaining ~4.9s polling a frontier nobody was going to pull, then deferred.

**Fix.** Re-request the urgent drain every 750ms while delivery stays pending, so an unproductive attempt gets several retries inside the same budget. Bounded well under `max_wait` and far coarser than the 100ms observe poll. A genuinely never-converging delivery still fails closed at `max_wait` — this loosens the retry cadence, not the safety boundary.

**How the two fixes relate.** `5b648f338` stopped controller pushes from being suppressed at the source (plugin side); this one stops the route path from giving up when its single nudge lands unproductively. You needed both: the first removes the common-case stall, the second removes the failure mode when the one-shot nudge races.

`make check` green (exit 0), new regression test asserts multiple retries while pending *and* that a never-converging delivery still fails closed. The existing one-shot test still passes unchanged, so we didn't trade correctness for retries. `make install-full` done — binary and plugin 0.2.278 installed to all 4 IDE roots.

Worth noting: the route half is a Rust change and is live now, but the plugin half still needs an IDE restart to load. Until you restart, you have the weaker of the two fixes — so if you see the deferral again before restarting, that's expected rather than a regression.

### Re: fix all issues identified this session — opus-4-8

Six commits shipped, and two items I deliberately did **not** fix because I could not verify the mechanism.

**Fixed and verified:**

- `#crdtpushdrain` (`5b648f338`, `6a2261b96`) — controller-published CRDT frontiers were suppressed by the plugin's speculative no-op drain backoff (escalates toward 30s on an idle document, exactly the Compact Exchange state), so every compact/finalize waited for the 2000ms `ack_recovery_force_refresh` escalation. Plus the route path latched its urgent drain to a single attempt.
- `#routeprogresswait` (`1659448e9`) — the recurring `route deferred ... delivery_pending for 5000ms`. `ops.log` shows `delivery_converged` *does* reach true moments later, and the JB action writes its prompt marker **first**, so route was deferring the convergence of a write the same action had just issued. The no-progress deadline now resets when the frontier advances; a frozen frontier still fails closed.
- `#committedwedge` (`c91bdf692`) — the circular deadlock. Hit it three times, most clearly on a pure `--backlog-add`: no response body, document matching HEAD, nothing that could duplicate, still refused. Pending-only closeouts now reopen from HEAD. This is what made recording these very backlog items possible.
- `#backlogqueueprepend` (`64a3e1d07`) — your queue-ordering report. The bare `queue` token hard-defaults to `Append`, and the `priority` attribute on the *same marker* never influenced the mode. Verified live: `(Append, 49)` → `(Prepend, 49)`.
- `#queuepersistloud` (`1659448e9`) — queue maintenance printed confident progress while *computing*, then swallowed persistence failure into a bare warning. That is why a filed backlog id was absent from the queue entirely rather than mis-ordered. Now says plainly that nothing was applied, as a structured warning.
- `#syncobscache` (`800d3c2fd`, tmux-router `7b5cedc`, `8957e619f`) — lazily `SlotMap` observation cache plus a shared pane-liveness snapshot.

**Where I was wrong, in the open:**

The focus-sync work did **not** fix the lag. I cut spawns 109→63 and `pane_dead` full scans 19→1, but wall clock did not move (min-of-5 2207ms vs 2031ms baseline; every run 2.0–2.4s). That falsifies my "spawn count IS the cost" premise. Instrumented phases sum to ~375ms of a ~2.5s sync, leaving ~2.1s in an unmeasured span I did not isolate. Filed as `#syncfocuslag` with the exact next step. I kept the caching because O(n) full scans → 1 is a real reduction with correct invalidation, not because it fixed anything.

I also reverted two speculative fixes rather than ship them: a response-path loosening for the stale-baseline wedge (it tripped the ambiguous-patchback guard, which correctly refuses a visible response with no capture artifact), and a preflight cycle-reuse change (preflight *deliberately* rolls the cycle — asserted by an existing test).

**Not fixed — `#jbsteerinterrupt`.** `attempt_busy_existing_pane_interrupt_recovery` is codex/opencode-only, so it cannot be what interrupted your Claude pane. Something else injected it and I don't know what. I won't edit safety-critical routing code on an unverified hypothesis, having already made and reverted that mistake twice today. I need `ops.log` plus route diagnostics captured at the moment of the interrupt on a Claude pane.

CI run 29653355392 was in progress at closeout. The pre-existing `finalize_skips_ignored_untracked_session_doc` failure is still red and still not reproducible locally after four attempts.
<!-- agent:boundary:d520485c -->
<!-- /agent:exchange -->
## Queue

<!-- agent:queue priority preset="#spec-test-build-install-commit-push" go -->
- do [#syncobscache2]
- ⏭️ do [#jblocalhistverify]
- ⏭️ do [#capturebacklogatomic]
- ⏭️ do [#opcapturelazily]
- ⏭️ do [#jbtsiftnosub2verify]
- 🚧 do [#5bzd]
- do [#sidecarstate]
- do [#64we]
- do [#adstatechart2]
- do [#adstatechart3]
- do [#adstatechartkt]
- do [#adstatechartjs]
- do [#ipcauthority]
- do [#ipcliveness]
- do [#ipcverhandshake]
- do [#ipcpostcommitinv]
- do [#staleplugindeploy]
- do [#5qjj]
- ⏭️ do [#jblocalhistverify]
- ⏭️ do [#capturebacklogatomic]
- ⏭️ do [#opcapturelazily]
<!-- /agent:queue -->

## Notes

<!-- agent:notes -->
We are not using an evented FD because this path deliberately has no live bidirectional connection to the plugin. The evented transport is the socket IPC path; file IPC exists specifically for “socket absent/degraded/stale but plugin file
watcher may still apply patches”. We could use notify/inotify/kqueue as an implementation detail for the filesystem wait, but it still needs the same timeout, nack, ack-content, committed-cycle, and post-apply validation fallback. The
right extraction is probably a focused FileIpcAckWaiter/sidecar waiter with today’s poll backend first, then an evented backend as an optimization once it is out of orchestration.
---
```
● Finalize deferred with an IPC edge case: socket reported already_applied (response landed in the live buffer) but
  disk_missing_response_probe failed and it deferred ("document changed after merge; retry after typing stops"). Per
  the closeout rules I must not force-disk here — I retry the editor path. Let me first check the actual on-disk state.

  Ran 1 shell command

● The response was captured (cycle at response_captured) and socket-applied to a live JetBrains buffer
  (already_applied), but not yet flushed to disk — a live agent-doc sync --focus process for this doc is holding the
  editor buffer, so the disk-probe deferred ("retry after typing stops"). Per closeout rules I retry the editor path
  (no force-disk). Let me retry the finalize:

  Ran 1 shell command

● Same deferral, and notice: current_len=3546 stayed constant but current_hash changed between the two attempts
  (02e1c199 → 64233287) — the live buffer is actively oscillating (my boundary insertions + a live sync --focus process
  for this doc). The response is durably captured (response_sha256=d7ddb028). This is the already_applied +
  buffer-differs deadlock. Before choosing recovery, I must know if there's a real JetBrains editor or just an orphaned
  sync process holding a phantom buffer.

  Ran 1 shell command

● No live JetBrains IDE, no active listener (both sockets 0B/stale), and the sync process has exited. The
  already_applied acks came from a now-dead stale endpoint — the "live buffer" is phantom. The response is durably
  captured and disk is still the clean 2344-byte prompt. This is the documented stale-endpoint → force-disk recovery
  (no live editor to preserve)
JB `Compact Exchange` on this document had an error. As I was typing this prompt a JB `File Cache Conflict` occurred. I lost what I was typing:
```
agent-doc command failed (exit 1): Error: project controller command `compact_document` failed: compact: editor delivery ACK recovery for /home/brian/work/btakita/agent-loop/tasks/agent-doc/agent-doc-bugs2.md did not settle within 8000ms; the canonical response remains retained in CRDT + Lazily state and the editor reconnect continues asynchronously (no force-disk or operator recovery required)
```.---
 Let me first confirm HEAD hasn't advanced (no concurrent commit), then force-disk
  finalize.
```
---
Please make backlog replacements for all gated review items. We need to complete the plan, not have any more gates after this round. Let me know when I need to install JB plugin updates. Make sure all JB live gate items have adequate logging to prove/disprove the item. Clear the review items. I will test, files bugs, and you will check the logs.
---
`'/clear' is disabled while a task is in progress
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
<!-- /agent:notes -->

## Backlog

<!-- agent:backlog priority queue -->
- [ ] [#syncfocuslag] #agent-doc-bug Focus-sync lag NOT fixed — premise falsified. Spawns cut 109->63 (pane_dead scans 19->1) via #syncobscache but wall clock did NOT move: min-of-5 2207ms vs 2031ms baseline; runs 2.0-2.4s throughout. Spawn count is not the bottleneck. Of a ~2.5s safe-passive sync, instrumented phases (sync_lock_wait 0ms, window_resolution 3-13ms, tmux_router 150-370ms) sum to ~375ms, leaving ~2.1s UNMEASURED between window_resolution and the tmux_router reconcile. NEXT: add permanent log_sync_latency phases inside that span to bisect it (a blind line-insert attempt landed mid-expression and was reverted). No production sleeps there, so it is real work.
- [ ] [#jbsteerinterrupt] #agent-doc-bug JB `Run Agent Doc` mid-turn showed Claude Code "Interrupted". MECHANISM UNVERIFIED — do not guess. attempt_busy_existing_pane_interrupt_recovery (agent-doc-route-io/src/busy_pane.rs:225) is codex/opencode-ONLY, so it cannot be the source for a Claude pane; something else injected it. NEEDED: ops.log + route diagnostics captured at the moment of interrupt on a CLAUDE pane before touching safety-critical routing code. Contract #realtime-steering-verbatim: the prompt is already in the document and the running turn should consume it as steering, so interrupting to deliver it is wrong by design.
- [ ] [#syncobscache2] #syncobscache follow-up (the rest of the focus-sync win): extend the command-scoped observation cache into ../tmux-router. After the agent-doc-side cache: 109 -> 80 subprocess spawns, 2225 -> 2031ms (~9% only). list-panes barely moved (35 -> 34) because tmux-router spawns from 47 sites directly via its own Command builder, bypassing TmuxCommandRunner. Dominant remainder: `list-panes -a -F #{pane_id} #{pane_dead}` run 18x identically — pane_dead_flag lists ALL panes and scans for one, once per pane. Fix: move the scope into tmux-router and/or batch pane_dead_flag into one map fetch. Re-measure with the PATH-shim spawn counter.
- [ ] [#jbrunsteering] #agent-doc-bug JB `Run Agent Doc` while a turn is already running should feed the prompt in as realtime steering (#realtime-steering-verbatim / #realtime-steering-aggregate) instead of interrupting the active turn. Operator-reported: invoking it mid-turn produced "Interrupted · What should Claude do instead?" rather than surfacing the new prompt to the running turn. NOT STARTED.
- [ ] [#crdtpushdrainverify] [operator-verify] #crdtpushdrain live proof: after an IDE restart loads JetBrains plugin 0.2.278, run Compact Exchange on a document that has been idle for >30s and confirm ops.log `compact_crdt_relay_acknowledged ... wait_ms=` drops well below the prior ~4.5s floor (expect roughly the pull/apply round trip), with no `compact_crdt_ack_recovery_signal ... reason=ack_recovery_force_refresh` line needed to unstick it. Restart is required because reload-lib only swaps the cdylib, not the Kotlin plugin.
- [ ] [#supboundarylivelock] #agent-doc-bug Supervisor idle-boundary recycle can livelock, leaving a stale binary mapped indefinitely. supervisor_recycle_action (agent-doc-supervisor/src/lifecycle.rs:124) returns None on `!turn_boundary` for everything except write_wedged/editor_delivery_stale, and turn_boundary = prompt_visible && !turn_active (idle_watch.rs:1898) never holds for a continuously-active pane. The only bypass, stale_recycle_safe_checkpoint (idle_watch.rs:136) = supervisor_stale && inflight_handlers == 0, is gated on staleness, so an explicit `admin recycle` on a NON-stale busy supervisor can never be consumed and silently expires at DEFAULT_RECYCLE_REQUEST_TTL_SECS=900 (recycle_request.rs:17) with the ledger phase left Requested. --force does not help: rpc.rs:5905 only changes the reason string, and idle_watch.rs:2403 collapses the request to a plain bool. MAX_CYCLE_OPEN_DEFER_TICKS escalation cannot rescue it either (idle_watch.rs:2253 only accrues the streak AT a boundary). OBSERVED 2026-07-18: supervisor pid 2040014 stayed stale across this whole session even though it IS stale (so the checkpoint bypass should apply and inflight_connection_handlers is an in-process RAII-guarded atomic that cannot leak) - the stale-side path needs its own trace. Fixing the non-stale side means a mid-turn execve, which is exactly what the boundary gate exists to prevent, so this needs a design decision (manufacture a boundary via the recycle-yield path? bound the defer? re-arm instead of expiring?), not a quick patch. Related shipped fixes: #dispatchgateleak + #authorityfactretention (submodule 566a72f6d).
- [ ] [#statedbvacuum] [recommended] #agent-doc-feature One-time reclaim + ongoing compaction for .agent-doc/state.db. #authorityfactretention (566a72f6d) bounds future growth (170,215 -> 5,000 document_authority_observed rows on the live db), but SQLite reuses freed pages rather than shrinking: agent-loop state.db is still 477MB on disk with ~47,000 free pages and state_events holding 288MB across 73,822 pages for only 11,056 rows. Reclaiming needs a VACUUM, which takes an exclusive lock and must not run under a live controller. Add a bounded maintenance path (an `agent-doc admin vacuum` gated on no live controller/supervisor, or PRAGMA incremental_vacuum wired into an idle boundary) instead of leaving operators to run sqlite3 by hand.
- [ ] [#qdonestrikeverify] #agent-doc-bug #qdonestrike-durable follow-up: confirm resolved queue heads actually clear once the project controller runs a build containing agent-doc submodule 4faed5b01. Fix shipped: persist_queue_maintenance_doc now drives the bounded ensure_document_model and re-observes before failing closed, so a missing/pending replica no longer discards the whole queue-maintenance plan (the auto-strike of agent:done heads was recomputed and thrown away every preflight). NOT yet provable live: controller serve pid 1501201 + route-owned supervisor pid 2040014 still map the pre-fix binary, and preflight now reports the honest underlying state (editor authority stayed in editor_attached_model_missing after a bounded observation; recovery=retry_without_disk_write). Controller kill was deliberately NOT done because it is project-scoped and tasks/professional/equityfundingsource.md is foreign-owned by a concurrent pane %31. Verify after an idle-boundary recycle: preflight should log queue_maintenance_model_ensure and the struck heads (📌 do [#jblocalhistverify], do [#5bzd]) should disappear from agent:queue instead of being re-logged as auto-struck every run.
- [ ] [#crashresiliencelograce] test-hygiene: crash_resilience::tests::log_panic_to_file_appends_without_panicking fails under a parallel workspace run (cargo test --workspace) at src/crash_resilience.rs:173 but passes in isolation and under make check. The panic-log append test shares a fixed path across concurrently-running crate test binaries. Give it a per-test temp path so the suite is order-independent.
- [ ] [#compacttombgc] [recommended] Add a fenced Compact Exchange CRDT epoch: rebootstrap attached replicas from the compacted snapshot, reject or component-rebase older-epoch deltas, expose the all-replica stability frontier, then run TextCrdt tombstone GC.
- [ ] [#compactindcellsverify] [operator-verify] With agent-doc 0.35.3 and JetBrains plugin 0.2.276 active, run Compact Exchange while deleting several queue items and editing a post-boundary prompt; confirm the deletions and prompt survive, compaction commits, and ops.log records compact_rebased_authoritative_siblings without compact_dropped_non_exchange_item.
- [ ] [#5qjj] steeringmirrorverify|[operator-verify] Restart IntelliJ IDEA 2026.1 to activate plugin 0.2.258 (if stale bytecode appears, uninstall, restart, reinstall the signed local zip, and restart); during an active turn, make multiple exchange edits and verify the JetBrains banner and VS Code status item report the aggregate count/full hover, then run agent-doc once after editing and confirm the handoff queues without submitting a partial edit.
- [ ] [#zqv8] [operator-verify] #jbrelaylive Restart IntelliJ IDEA 2026.1 to activate the installed JetBrains plugin 0.2.247, rerun Run Agent Doc on tasks/professional/equityfundingsource.md while the dim Summarize recent commits suggestion is visible, and confirm dispatch succeeds without a protected-input refusal or a long UI freeze.
- [ ] [#crde] JetBrains Run Agent Doc route/dispatch failed after bounded submit/start proof retries #jbrunautobug #agent-doc-bug failure_class=accepted_without_dispatch_start_proof document=/home/brian/work/btakita/agent-loop/tasks/professional/equityfundingsource.md stage=dispatch_only_dispatch_start_proof pane=%83 actor_generation=1214 editor_attempt_id=1783978030079-4 dispatch_proof_state=accepted_without_dispatch_start_proof elapsed_ms=10000 diagnostic_path=none ops_log_path=/home/brian/work/btakita/agent-loop/.agent-doc/logs/ops.log ops_log_marker=route_submit_issue(issue=accepted_without_dispatch_start_proof,phase=dispatch_only_dispatch_start_proof,result=accepted_without_dispatch_start_proof) [symptom-key invariant=run_agent_doc_route_dispatch_failure document=71b0db3f68b7c54571017f333b0989d1db07efa4e5ecf5ec73437e2e857072ad component=route/dispatch_only_dispatch_start_proof content_hash=sha256:3251e072b5c1857ba06e9ba613e1004b5eefef103db7003d7dcf7efe0d698ea8]
  evidence: JetBrains Run Agent Doc route/dispatch failed after bounded submit/start proof retries #jbrunautobug #agent-doc-bug failure_class=accepted_without_dispatch_start_proof document=/home/brian/work/btakita/agent-loop/tasks/professional/equityfundingsource.md stage=dispatch_only_dispatch_start_proof pane=%83 actor_generation=1214 editor_attempt_id=1783978020608-2 dispatch_proof_state=accepted_without_dispatch_start_proof elapsed_ms=10000 diagnostic_path=none ops_log_path=/home/brian/work/btakita/agent-loop/.agent-doc/logs/ops.log ops_log_marker=route_submit_issue(issue=accepted_without_dispatch_start_proof,phase=dispatch_only_dispatch_start_proof,result=accepted_without_dispatch_start_proof)
- [ ] [#steeringobservableset] #agent-doc-bug Phase 6 durable graph adoption: back RealtimeSteeringSet with a lazily reactive OBSERVABLE SET (not a drain-one-head QueueCell) keyed on the body-aware identity (Phase 3.1), reconciled on the reliable-sync plane; session-check reads set-emptiness (all directives settled) instead of re-diffing HEAD↔buffer. Plan: src/agent-doc/tasks/agent-doc/plan-exchange-tree-seqcrdt-and-ipc-unify.md (Phase 6). [recommended]
- [ ] [#capturebacklogatomic] [focused-cycle] #agent-doc-bug A finalize that captures the response but fails before recording backlog mutations strands in response_captured with requires_backlog_capture=true + pending_added_this_cycle=false; preflight then refuses closeout and errors "still response_captured after recovery", forcing a manual mv-aside because replaying the capture re-trips the backlog gate. Fix: capture the whole cycle intent (response + backlog mutations) atomically, and let preflight supersede an uncommittable capture instead of looping. Exposed by this turn. Plan: tasks/agent-doc/plan-capture-includes-backlog-mutations.md
- [ ] [#opcapturelazily] [focused-cycle] #sidecarstate op-capture (.agent-doc/op-capture/<doc-hash>.json, agent-doc-op-capture-io #qnodemerge4) is a bespoke JSON sidecar source of truth: durable ordered EditorOps + base_hash that preflight repair / CRDT merge REPLAY as authority then consume+clear (base-hash-guarded so safe). Because it is a separate replay source, clearing captures/ + state/cycles/ aside did not stop the churn this turn. Migrate onto the lazily state backbone (co-located with cycle-phase state) so captured-op authority is one coherent atomically-clearable model. 96-ref cross-crate surface; folds into #sidecarstate. Plan: tasks/agent-doc/plan-op-capture-lazily-state-migration.md
- [ ] [#jbtsiftnosub2verify] [operator-verify] #jbtsiftnosub2 live proof (0.34.74): with NO tmux pane for a doc, trigger JB Run Agent Doc and confirm the reopen trigger SUBMITS (or auto-resubmits) instead of sitting typed-but-unsubmitted; ops.log shows fresh_route_stranded_trigger_resubmit + fresh_route_start_acknowledged_after_resubmit; a stuck submit now fails closed with a visible FreshStart startup-miss.
- [ ] [#sidecarstate] [focused-cycle] #agent-doc-bug Replace hot-path snapshot/cycle/live-buffer sidecars with the lazily state backbone for document authority, lifecycle recovery, and crash-reentrant pipeline stages; auto-start/reconcile the document model before preflight exposes missing CRDT replica errors; reconcile drift inside agent-doc on re-entry. Plan: tasks/agent-doc/plan-lazily-state-backbone-sidecar-retirement.md
- [ ] [#64we] JetBrains Run Agent Doc route/dispatch failed after bounded submit/start proof retries #jbrunautobug #agent-doc-bug failure_class=accepted_without_dispatch_start_proof document=tasks/professional/equityfundingsource.md stage=supervisor_dispatch_start_proof pane=%156 actor_generation=1039 editor_attempt_id=1783140065173-1 dispatch_proof_state=none elapsed_ms=10134 diagnostic_path=/home/brian/work/btakita/agent-loop/.agent-doc/logs/route-submit/1783140083790-supervisor_dispatch_start_unproven-codex-_156-ad5c195b73fc.txt ops_log_path=/home/brian/work/btakita/agent-loop/.agent-doc/logs/ops.log ops_log_marker=route_submit_issue(issue=accepted_without_dispatch_start_proof,phase=supervisor_dispatch_start_proof,result=accepted_without_dispatch_start_proof) [symptom-key invariant=run_agent_doc_route_dispatch_failure document=71b0db3f68b7c54571017f333b0989d1db07efa4e5ecf5ec73437e2e857072ad component=route/supervisor_dispatch_start_proof content_hash=sha256:503e36d813c831fce9bfe367973c345adce5a2f2620dccc4fa0be6e4363b5087]
- [ ] [#adstatechart2] [focused-cycle] #agent-doc-feature Phase E rung 2 (observability): wire session-check/status to READ adstatechart configuration() as an advisory named-snapshot (transport.x editor_sync.x closeout.x supervisor) alongside today's ops.log markers. No behavior change to closeout. Guard semantics must match whatever A-D shipped. PRD: tasks/agent-doc/prd-adstatechart-local-process-statechart.md
- [ ] [#adstatechart3] [focused-cycle] #agent-doc-feature Phase E rung 3 (load-bearing): switch adstatechart to the typed lazily 0.19 ChartBuilder, bump agent-doc-state-backbone to lazily 0.19, use ThreadSafeStateChart for cross-thread status observation, and replace the ad-hoc editor_synced check at the finalize/commit site with chart send(commit, guards) plus a fail-closed fallback when the feature is off. SimWorld: N wedged-finalize->direct-commit cycles never commit from stale disk. Live editor proof gated behind decomposition + install. PRD rung 3.
- [ ] [#adstatechartkt] [focused-cycle] lazily-kt 0.9.0 SHIPPED (ChartBuilder + ThreadSafeStateChart + shared fromStates + context-free engine; GitHub Packages via release.yml, tag v0.9.0; full test suite green). REMAINING: bump the JetBrains plugin's lazily-kt dependency to 0.9.0 and consume ChartBuilder/ThreadSafeStateChart for the merge-ownership state machine (#mergestatemachine3 fold-in).
- [ ] [#adstatechartjs] [focused-cycle] lazily-js 1.1.0 SHIPPED (ChartBuilder + statechart.d.ts builder types + shared fromStates; tag v1.1.0; 169/169 tests green). No thread-safe/async chart in js (no reactive Context, no threads). REMAINING: npm publish is operator-2FA-gated (#lzjsnpmpub: npm login then npm publish --access public), then bump the VS Code plugin @lazily/js dep to 1.1.0 and consume ChartBuilder.
- [ ] [#ipcauthority] [focused-cycle] #agent-doc-bug IPC root-cause A (data-loss authority ambiguity). SUPERSEDED PREMISE: the prior "DONE this pass" durable recovery sidecar (commit 317a336f, write_policy::content_ours_drops_operator_text + agent_doc_fs::preserve_dropped_operator_buffer -> .agent-doc/recovery/) has since been REMOVED with the sidecar retirement; do not treat it as landed safety net. REMAINING (now the whole item): make the CRDT/reliable-sync plane the single closeout authority and demote content_ours/snapshot to audit-only, so the operator-wins merge needs no sidecar fallback. Relates to #sidecarstate.
- [ ] [#ipcliveness] [focused-cycle] #agent-doc-bug IPC root-cause D (transport/state liveness). DONE this pass: corrupt controller-state auto-quarantine (commit e06301e6) — agent_doc_fs::read_valid_or_quarantine wired into project_controller::read_bootstrap; a 0-byte/unparseable controller-state.json is moved aside + re-bootstrapped instead of wedging start (class 11 auto-recovery). REMAINING (blocked on decomposition landing, dirty files): editor IPC listener read_line no timeout (fd/thread leak on half-open client), connect_sync no connect timeout, file-IPC no negative-ack. Touches ipc_socket.rs, write/ipc/transport.rs, agent-doc-ipc-io, editor plugins.
- [ ] [#ipcverhandshake] [focused-cycle] #agent-doc-bug IPC root-cause B: cross-process version skew has no negotiation (staleness inferred from mtime/inode; drives stale_install + stale-supervisor CRDT corruption). Extends the shipped #stale-plugin-detect. Fix: add a build-id/protocol-version field to the IPC handshake (agent-doc-ipc-protocol) so a newer finalize client refuses or forces-reload on mismatch. status.rs already carries injected_binary_version to extend to the wire.
- [ ] [#ipcpostcommitinv] [focused-cycle] #agent-doc-bug IPC root-cause C: structural invariants enforced on wrong write path. #boundaryaccum root fix landed (git.rs unconditional snapshot collapse) but needs a POST-COMMIT verification gate that fails loudly/self-heals if HEAD carries more than one boundary or a duplicated scaffold. Reuse ipc_corruption.rs detect_duplicated_scaffold. Touches git.rs, ipc_corruption.rs, flow/closeout.rs. Plan: tasks/agent-doc/plan-boundary-accumulation-committed-blob.md.
- [ ] [#staleplugindeploy] After the module-decomposition refactor lands: make install + push the shipped #stale-plugin-detect change (submodule commit 9b38d017). Deferred this cycle: installing would deploy the uncommitted decomposition WIP as the live binary. Verify a live stale plugin then surfaces the stale_plugin preflight warning.
- [ ] [#vscsaveproj] #agent-doc-bug VS Code save_document observability parity: processSaveDocumentSignal (editors/vscode/src/extension.ts) only writes outcomes to outputChannel, while JB saveDocumentViaDocument records StateProjectionBridge surface ops for missing_file/missing_document/saved/failed. Add the VS Code state-projection surface-op analogue so save_document outcomes are observable in the state mirror (not just the log), plus a behavioral test driving processSaveDocumentSignal end-to-end (open doc -> signal -> save -> ack-content).
- [ ] [#restartbleednonroute] #agent-doc-bug Harden auto-install child stdio against fd2==pane: run_auto_install_steps_once (project_controller/rpc.rs) blindly dups fd2 via auto_install_child_stdio_plan(); when SupervisorStderrRedirect is inactive (non-route-owned TUI, or route-owned but maybe_start fell back to inactive on error) fd2 is the agent pane and make-install output still bleeds. Open the session project_root .agent-doc/logs/supervisor-stderr.log and pass its fd to auto_install_child_stdio_plan_to_fd(log_fd) so build output is off-pane regardless of route ownership; add a redirect-inactive regression.
- [ ] [#r4gq] [operator-verify] Live JB Run Agent Doc dispatch-hang proof (#kjw0 fix, shipped 0.34.66 build): after the recycled build is running, trigger Run Agent Doc into a BUSY codex pane (mid-turn, 'esc to interrupt' visible) and confirm it returns FAST with a queued-behind-active-turn outcome (pane shows queued status) instead of hanging ~21s then a dispatch-start-unproven notification. Grep ops.log for route_dispatch_start_queued_behind_active_turn (present on success) and confirm accepted_without_dispatch_start_proof is ABSENT for the busy case. Root cause: route injected the trigger into a mid-turn pane; codex queued it behind the active turn so the UserPromptSubmit hook could not advance within the 10s proof budget, and the direct-pane path paid TWO 10s waits (initial + late-resubmit) then filed a false bug. Fix: short_circuit_dispatch_start_when_pane_busy in route/dispatch.rs detects busy_proof_line and returns RoutedDispatchStartProof::AcceptedQueuedBehindActiveTurn (accepted, not fail-closed).
- [ ] [#af88] [focused-cycle] #agent-doc-bug IPC wedge hardening follow-ups from the 2026-07-01 audit (wedges C + A FIXED this cycle: supervisor IPC listener now spawns per-connection threads so a half-open client cannot freeze all supervisor dispatch; editor IPC send now has set_send_timeout). Remaining: (E) pty.rs stdin.lock() held across the blocking read — evaluate a raw-fd read that preserves the restart/quit-prompt stdin coexistence in start.rs; (B/D) editor IPC listener per-connection read_line has no timeout (thread/fd leak under a persistent half-open client) — add a bounded backstop; (F) connect_sync has no connect timeout (mitigated today by the 3s wait_for_controller deadline loop). Also decide the AGENTS.md 'No async' convention nuance for the server-scale roadmap.
- [ ] [#claimnewpane] [focused-cycle] #agent-doc-feature Claim for Tmux Pane should open a new document in a fresh pane in the SAME tmux session (never replace the current pane or move the window to another session). Plan: tasks/agent-doc/plan-claim-new-pane-same-session.md
- [ ] [#boundaryaccum1] [focused-cycle] #agent-doc-bug Scattered agent:boundary markers accumulate in committed content (3 in this doc: d0dd7c4e, qactsync1, ftimm0001) — violates the exactly-one-boundary invariant. Root cause: reposition_boundary_in_snapshot (git.rs:1613) hands the collapse to the finalize/IPC path via the active-run guard AND delegates the working-tree rewrite to the IDE plugin via the IPC-listener guard; when finalize wedges on stale_install and the response lands via direct commit (and the stale plugin never applies the IPC reposition signal), nothing re-enforces the single-boundary invariant on the binary's own staged/committed blob, so one boundary accretes per wedged cycle. Fix: always collapse boundaries in the binary-owned snapshot/staged blob independent of the guards (git stages a snapshot, not the live editor buffer, so it never races user text) and scope the guards to the working-tree disk rewrite only; add a post-commit invariant check that reconciles or fails loudly if HEAD carries >1 boundary; promote the aspirational reposition_boundary_removes_stale/apply_patches_boundary_invariant tests to real regressions + SimWorld coverage for N wedged-finalize→direct-commit cycles. Distinct from #fullboundary (convergence gate reduces the wedge trigger; this makes the invariant self-healing regardless of write path). Plan: tasks/agent-doc/plan-boundary-accumulation-committed-blob.md
- [ ] [#qactsync] [focused-cycle] #agent-doc-feature Bidirectional realtime sync of queue-activation state: when the operator adds/removes go on agent:queue OR changes the queue: frontmatter value, the realtime system converges BOTH representations to the operator edit (operator-authoritative, never revert). Today explicit_queue_go_mode reads both but they are not synced, so halting needs editing both (go-marker churn). Define a canonical QueueActivation state + edit-provenance propagation; deterministic on two-sided conflict; fixed-point on synced input. SimWorld both directions + conflict; make tmux-ci + live editor proof. Plan: tasks/agent-doc/plan-queue-activation-state-sync.md
- [ ] 🚧 [#ftimmediate] #agent-doc-feature Free-text prompts begin work immediately: a free-text exchange/queue prompt is an executable directive that should start work at once; if the agent determines a dedicated/fresh session is needed (high accretion or focused-cycle scope), it should auto /clear or request a PCP restart and RUN the work, not park it as an operator-gated defer. Tighten the #drain-no-defer / #qfocsup boundary so focused-cycle free-text prompts get run-after-clear rather than left for the operator.
- [ ] [#qeditrace] [focused-cycle] #agent-doc-bug Pre-run queue edits partially reverted: when the operator adds multiple queue heads before a run (or JB Run Agent Doc) with no active turn, only the FIRST added head survives and the rest revert to the pre-edit queue. Distinct from the during-turn race - the preflight queue-maintenance reconcile drops all-but-first operator addition. Investigate preflight/maintenance.rs: queue_journal record + record_live_buffer (#qftloss mode-6), #mirrorall backlog-to-queue mirror, #rz3a freshly-added hold-out, #7r2s manual-add pin, #qdedupsync dedup - hypothesis is convergence keeps the first new head (pin/anchor match) but journals-without-replaying the rest. Repro 2026-07-01 reitrades.md; live dup heads in bugs2 (do #tmuxsynccrashlog x2, do #60ev x2). Add SimWorld coverage for multi-line pre-run operator-add survival + fail-closed recoverable artifact. Plan: tasks/agent-doc/plan-queue-edit-race-consolidated.md
- [ ] [#jbcompactcommit] [operator-verify] Live JB Compact Exchange no-uncommitted-changes proof: after the fixed binary is installed + recycled, run JB Compact Exchange on a large session doc and confirm HEAD lands the compacted content (no green git gutter / no snapshot>visible drift on next preflight). Grep ops.log for compact_commit_head_mismatch (must be ABSENT on success; present = the new fail-closed guard fired with a reset --from-current recovery). Rust fix shipped in agent-doc-orchestration/src/compact.rs commit_compacted_authoritative (#jb-compact-commit-left-uncommitted).
- [ ] [#tmuxsynccrashlog] [operator-verify] Replacement for #tmuxsynccrash: JB Sync Tmux Layout live repro log review. After the operator runs repeated JB Sync Tmux Layout against the repro layout, inspect idea.log, agent-doc ops.log, and tmux server pid/session evidence; close only if the server pid stays stable with repair_layout no-op/passive supersede markers, otherwise file a follow-up bug with the failing log markers. Plan: tasks/agent-doc/plan-sync-tmux-layout-crash.md
- [ ] [#60ev] plan-dynamic-context-injection-ledger.md
- [ ] [#2y8j] [operator-verify] #qbasehashmemo live typing proof: after the supervisor auto-installs 0.34.49 and recycles, type a sustained burst in a large doc (equityfundingsource.md or agent-doc-bugs2.md) in the JB editor and confirm typing is smooth; grep ops.log to confirm crdt_merge_base no longer fires per keystroke during a typing burst.
- [ ] [#cdtcutover] [focused-cycle] #agent-doc-feature FUTURE/SEPARATE rung — wire the evented CellDocTree model into the live finalize/write path (replace whole-tree reconcile at finalize with update_to per-node deltas; expose SemTree queries to preflight/supervisor drainable computations). Large, live-supervisor-critical, gated, own focused cycle + operator live proof. Plan: tasks/agent-doc/plan-celldoctree-evented-reflection.md
- [ ] [#coredecomp] [focused-cycle] Keep the former `agent-doc-core` God-crate surface decomposed into owned pure crates. Audit `pending.rs`, `queue_item_lifecycle.rs`, `template.rs`, `crdt_sync.rs`, `component.rs`, and related modules; move element-local semantics into `agent-doc-element-*`, document composition into `agent-doc-document`, merge/CRDT merge policy into `agent-doc-merge`, and FFI ABI into `agent-doc-ffi`. Do not recreate `agent-doc-core` as a compatibility facade or migration alias, and do not move runtime IO, snapshots, commits, IPC, tmux, or controller effects into pure crates.
- [ ] [#orchdecomp] [focused-cycle] Decompose the `agent-doc-orchestration` God crate along effect boundaries. Audit turn lifecycle, document realtime scheduling, queue/exchange lifecycle, closeout/commit, editor IPC, supervisor/controller state, tmux integration, recovery/repair, and session routing; move stable responsibilities into `agent-doc-turn`, `agent-doc-document`/document-realtime, `agent-doc-supervisor`/controller-state crates, `agent-doc-editor`, `agent-doc-tmux`, and narrow closeout/recovery crates as appropriate. Leave `agent-doc-orchestration` as a shrinking effect adapter only, not a deprecated compatibility facade; callers should move directly to the owning focused crate and pure merge/document code should stay out of orchestration.
- [ ] [#skillharnessplugin] [focused-cycle] Create the agent-doc integration plugin for `skill-harness`: implement an agent-doc-owned `SkillHarnessPlugin` adapter, define its detection and skill-path contract without depending on `agent-doc-core`, register/document the integration path, and add tests proving `skill-harness` can resolve agent-doc through the plugin boundary after `agent-doc-core` removal.
- [ ] [#crdtauth6] [operator-verify] CRDT authority live proof: verify the existing JB DocumentListener -> CrdtReplicaForwarder.forwardLocalDelta path, Kotlin Unix-socket ReplicaTransport, VS Code koffi replica seam, and Rust fan-out stay connected in a live editor session. This is proof/parity work only; operator-visible text is already authoritative and must not wait on this item. Plan: tasks/agent-doc/plan-crdt-authority-model.md
- [ ] [#nsfz] [operator-verify] #startupdeadline live dogfood: drive a real harness that never reaches a dispatch-ready prompt (e.g. auth-walled/hung TUI) and confirm the session fails closed within 30s with a 'session did not become dispatch-ready in Ns' stderr diagnostic + a recorded startup_miss marker, instead of hanging. Code-complete + tested (make check 5101 green, release build green); needs a live editor/pane eyeball.
- [ ] [#suprestassoc-live] [operator-verify] Restart the `tasks/professional/equityfundingsource.md` supervisor in a live editor/tmux setup and verify the actor/admin projection keeps EFS bound to its own pane/session without re-associating to `tasks/agent-doc/agent-doc-bugs2.md`.
- [ ] [#agentsignals] [future/separate] Add a realtime agent:signals element after core realtime convergence is stable. Start with deterministic built-in signals for tests, queue, supervisor, and ipc; design custom user-defined signals as plugin-backed tracked work with Rust as the first target and future multi-language support; use tasks/software/realtime-signals-patchboard.md as the GUI/semi-programmatic reference; keep MLP enhancement optional/experimental and not load-bearing for deterministic states.
- [ ] [#elementgraphprd] [future/separate] Define the single-document element layout for both single-model sessions and multi-harness orchestration, including typed graph/workers/jobs/schedule elements plus an advisory perceptron graph that scores visible signals without directly mutating runnable work. PRD: tasks/agent-doc/prd-element-layout-perceptron-graph.md
<!-- /agent:backlog -->

## Review
 
<!-- agent:review -->
- [/] [#falsetyping-guard] #agent-doc-bug provenance-aware visible-write guard. INTEGRATED to main (merge on 5bcfc07b) + binary/cdylib installed + recycled; guard tests 7/7 green; JB 0.2.205 zip + VS Code 0.2.38 compiled. REMAINING (operator): install JB plugin zip (build/distributions/agent-doc-jetbrains-0.2.205.zip) + reload IDE, install VS Code 0.2.38, then live proof — type during replica churn (no wedge) + genuine unsaved edit still fails closed. NOTE: bugs2.md can still wedge until the IDE plugin is reloaded (loaded plugin still calls FFI v2 → conservative fail-closed).
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
- [ ] [#exttasksources] #agent-doc-feature Add external task source adapters that project PM-tool tasks into the active document/work graph as source-backed nodes, keeping local queue/closeout authority document-owned and making remote push-back explicit and audited. PRD: tasks/agent-doc/prd-external-task-sources.md
<!-- /agent:icebox -->

## Completed / Reaped

<!-- agent:done archive=tasks/agent-doc-bugs2.done.md -->

<!-- completed work archived in tasks/agent-doc-bugs2.done.md -->
<!-- /agent:done -->
