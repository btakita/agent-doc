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

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7a9b103bd5b68223dac05a4c449ac03899aba53d6022e82100578b5b0-20260702-045815.md`*

Compacted content:
- Archived 2 response topic(s): #restartstderrbleed; IPC proof diagnostic (interrupted-cycle recovery)
- Prior summary/context: prior compacted content: Archived 3 response topic(s): next-steps to fix bugs (and "clear the review items"); wedge r...

### Re: ensure VS Code parity — publish_live_buffer editor-save (update spec + tests + install + recycle) — opus-4-8

Done. Restored VS Code parity for the editor-save / live-buffer flush, updated spec + tests, installed, and recycled.

**The gap (a real bug, `#vscodepublishparity`).** The editor-sync-barrier timeout flush (`settle_or_flush_editor_sync_barrier`, `crdt_relay_host.rs`) sent `publish_live_buffer` over the **socket only** and early-returned `cause=no_ipc_listener` when no socket owned the project. VS Code (and pluginless / file-IPC editors) run **no socket**, so those sessions silently missed the live-buffer publish a JetBrains session receives — even though the sibling convergence path (`live_buffer_delivery_missing_operator_text_authority_after_refresh`, `write/converge.rs`) already fell back to the `.agent-doc/patches/publish-live-buffer.signal` file signal. It is the same "call editor save" recovery you just used — VS Code just never got asked.

**Fix (committed `a435d7bd`).** Mirrored the converge fallback: socket when a listener is active, else the file signal VS Code watches. VS Code's handler (`processPublishLiveBufferSignal` → `publishLiveBufferNow`, already carrying the `#falsetyping-guard` provenance) was already correct — the binary just wasn't sending it. So parity is restored **binary-side**; no VS Code code change was needed (the fix ships to both plugins via the shared cdylib FFI).

**Spec + tests + install + recycle.**
- `SPEC.md`: documented the transport-symmetric `publish_live_buffer` contract (socket → file-signal fallback), matching the `save_document` rule.
- Test: `editor_sync_barrier_timeout_falls_back_to_publish_live_buffer_file_signal_without_listener` proves the no-listener path writes a read-only (no `patch_id`) signal. `make check` green.
- Install: `make install` (binary + cdylib @ 0.34.67); recycle marked 2 controllers + 3 route-owned supervisors at the idle boundary.

**Residual parity (from a full JB↔VS Code diff).** Filed `#vscsaveproj` for the one remaining actionable gap: VS Code's `save_document` outcome only logs to the output channel, whereas JB records StateProjectionBridge surface ops (observability parity). Two divergences left intentionally: VS Code republishes the live buffer after a successful save (JB doesn't — harmless), and VS Code has no negative-ack channel for save failure (by design — the binary discovers it via ack-content poll timeout).
<!-- agent:boundary:79dc58c4 -->
<!-- /agent:exchange -->
## Queue


<!-- agent:queue priority preset="#spec-test-build-install-commit-push" go -->
- ~~:round_pushpin: do [#falsetyping-guard]~~
- [#claimnewpane]
- [#boundaryaccum1]
- [#qactsync]
- 🚧 :pushpin: do [#restartstderrbleed]
Error: timed out waiting for project controller at /home/brian/work/btakita/agent-loop/.agent-doc/controller.sock`
- I saw serious bleeding into Claude Code during a previous restart. Also, a split pane was opened with Claude Code `/usage` somehow called.
- :pushpin: do [#jbcompactcommit]
- ~~:pushpin: do [#kjw0]~~
- :pushpin: do [#2y8j]
- :pushpin: do [#cdtcutover]
- :pushpin: do [#crdtauth6]
- :pushpin: do [#nsfz]
- :pushpin: do [#kj5flive]
- :pushpin: do [#tmuxsynccrashlog]
- :pushpin: do [#tmuxsynccrashlog]
- :pushpin: do [#60ev]
- :pushpin: do [#suprestassoc-live]
- :pushpin: do [#agentsignals]
- :pushpin: do [#qftlossdelta]
- :pushpin: do [#coredecomp]
- :pushpin: do [#orchdecomp]
- :pushpin: do [#skillharnessplugin]
- :pushpin: do [#elementgraphprd]
- :pushpin: do [#60ev]
- [#ftimmediate]
- do [#r4gq]
- do [#af88]
- do [#qeditrace]
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
- [ ] [#vscsaveproj] #agent-doc-bug VS Code save_document observability parity: processSaveDocumentSignal (editors/vscode/src/extension.ts) only writes outcomes to outputChannel, while JB saveDocumentViaDocument records StateProjectionBridge surface ops for missing_file/missing_document/saved/failed. Add the VS Code state-projection surface-op analogue so save_document outcomes are observable in the state mirror (not just the log), plus a behavioral test driving processSaveDocumentSignal end-to-end (open doc -> signal -> save -> ack-content).
- [ ] [#restartbleednonroute] #agent-doc-bug Harden auto-install child stdio against fd2==pane: run_auto_install_steps_once (project_controller/rpc.rs) blindly dups fd2 via auto_install_child_stdio_plan(); when SupervisorStderrRedirect is inactive (non-route-owned TUI, or route-owned but maybe_start fell back to inactive on error) fd2 is the agent pane and make-install output still bleeds. Open the session project_root .agent-doc/logs/supervisor-stderr.log and pass its fd to auto_install_child_stdio_plan_to_fd(log_fd) so build output is off-pane regardless of route ownership; add a redirect-inactive regression.
- [ ] [#gd88] [operator-verify] #restartstderrbleed live re-proof on a fixed binary: after the supervisor rebuilds onto a build containing auto_install_child_stdio, trigger a restart/auto-install idle boundary and confirm the agent pane shows NO make/cargo bleed, NO tmux split-window, and NO submitted /usage.
- [ ] [#r4gq] [operator-verify] Live JB Run Agent Doc dispatch-hang proof (#kjw0 fix, shipped 0.34.66 build): after the recycled build is running, trigger Run Agent Doc into a BUSY codex pane (mid-turn, 'esc to interrupt' visible) and confirm it returns FAST with a queued-behind-active-turn outcome (pane shows queued status) instead of hanging ~21s then a dispatch-start-unproven notification. Grep ops.log for route_dispatch_start_queued_behind_active_turn (present on success) and confirm accepted_without_dispatch_start_proof is ABSENT for the busy case. Root cause: route injected the trigger into a mid-turn pane; codex queued it behind the active turn so the UserPromptSubmit hook could not advance within the 10s proof budget, and the direct-pane path paid TWO 10s waits (initial + late-resubmit) then filed a false bug. Fix: short_circuit_dispatch_start_when_pane_busy in route/dispatch.rs detects busy_proof_line and returns RoutedDispatchStartProof::AcceptedQueuedBehindActiveTurn (accepted, not fail-closed).
- [ ] [#af88] [focused-cycle] #agent-doc-bug IPC wedge hardening follow-ups from the 2026-07-01 audit (wedges C + A FIXED this cycle: supervisor IPC listener now spawns per-connection threads so a half-open client cannot freeze all supervisor dispatch; editor IPC send now has set_send_timeout). Remaining: (E) pty.rs stdin.lock() held across the blocking read — evaluate a raw-fd read that preserves the restart/quit-prompt stdin coexistence in start.rs; (B/D) editor IPC listener per-connection read_line has no timeout (thread/fd leak under a persistent half-open client) — add a bounded backstop; (F) connect_sync has no connect timeout (mitigated today by the 3s wait_for_controller deadline loop). Also decide the AGENTS.md 'No async' convention nuance for the server-scale roadmap.
- [ ] [#claimnewpane] [focused-cycle] #agent-doc-feature Claim for Tmux Pane should open a new document in a fresh pane in the SAME tmux session (never replace the current pane or move the window to another session). Plan: tasks/agent-doc/plan-claim-new-pane-same-session.md
- [ ] [#boundaryaccum1] [focused-cycle] #agent-doc-bug Scattered agent:boundary markers accumulate in committed content (3 in this doc: d0dd7c4e, qactsync1, ftimm0001) — violates the exactly-one-boundary invariant. Root cause: reposition_boundary_in_snapshot (git.rs:1613) hands the collapse to the finalize/IPC path via the active-run guard AND delegates the working-tree rewrite to the IDE plugin via the IPC-listener guard; when finalize wedges on stale_install and the response lands via direct commit (and the stale plugin never applies the IPC reposition signal), nothing re-enforces the single-boundary invariant on the binary's own staged/committed blob, so one boundary accretes per wedged cycle. Fix: always collapse boundaries in the binary-owned snapshot/staged blob independent of the guards (git stages a snapshot, not the live editor buffer, so it never races user text) and scope the guards to the working-tree disk rewrite only; add a post-commit invariant check that reconciles or fails loudly if HEAD carries >1 boundary; promote the aspirational reposition_boundary_removes_stale/apply_patches_boundary_invariant tests to real regressions + SimWorld coverage for N wedged-finalize→direct-commit cycles. Distinct from #fullboundary (convergence gate reduces the wedge trigger; this makes the invariant self-healing regardless of write path). Plan: tasks/agent-doc/plan-boundary-accumulation-committed-blob.md
- [ ] [#qactsync] [focused-cycle] #agent-doc-feature Bidirectional realtime sync of queue-activation state: when the operator adds/removes go on agent:queue OR changes the queue: frontmatter value, the realtime system converges BOTH representations to the operator edit (operator-authoritative, never revert). Today explicit_queue_go_mode reads both but they are not synced, so halting needs editing both (go-marker churn). Define a canonical QueueActivation state + edit-provenance propagation; deterministic on two-sided conflict; fixed-point on synced input. SimWorld both directions + conflict; make tmux-ci + live editor proof. Plan: tasks/agent-doc/plan-queue-activation-state-sync.md
- [ ] 🚧 [#ftimmediate] #agent-doc-feature Free-text prompts begin work immediately: a free-text exchange/queue prompt is an executable directive that should start work at once; if the agent determines a dedicated/fresh session is needed (high accretion or focused-cycle scope), it should auto /clear or request a PCP restart and RUN the work, not park it as an operator-gated defer. Tighten the #drain-no-defer / #qfocsup boundary so focused-cycle free-text prompts get run-after-clear rather than left for the operator.
- [ ] [#qeditrace] [focused-cycle] #agent-doc-bug Pre-run queue edits partially reverted: when the operator adds multiple queue heads before a run (or JB Run Agent Doc) with no active turn, only the FIRST added head survives and the rest revert to the pre-edit queue. Distinct from the during-turn race - the preflight queue-maintenance reconcile drops all-but-first operator addition. Investigate preflight/maintenance.rs: queue_journal record + record_live_buffer (#qftloss mode-6), #mirrorall backlog-to-queue mirror, #rz3a freshly-added hold-out, #7r2s manual-add pin, #qdedupsync dedup - hypothesis is convergence keeps the first new head (pin/anchor match) but journals-without-replaying the rest. Repro 2026-07-01 reitrades.md; live dup heads in bugs2 (do #tmuxsynccrashlog x2, do #60ev x2). Add SimWorld coverage for multi-line pre-run operator-add survival + fail-closed recoverable artifact. Plan: tasks/agent-doc/plan-queue-edit-race-consolidated.md
- [ ] [#jbcompactcommit] [operator-verify] Live JB Compact Exchange no-uncommitted-changes proof: after the fixed binary is installed + recycled, run JB Compact Exchange on a large session doc and confirm HEAD lands the compacted content (no green git gutter / no snapshot>visible drift on next preflight). Grep ops.log for compact_commit_head_mismatch (must be ABSENT on success; present = the new fail-closed guard fired with a reset --from-current recovery). Rust fix shipped in agent-doc-orchestration/src/compact.rs commit_compacted_authoritative (#jb-compact-commit-left-uncommitted).
- [ ] [#qftlossdelta] #agent-doc-bug focused invariant-loss follow-up from failed #qftlossproof live proof: `.agent-doc/logs/ops.log:4338964-4338997` shows ACK/live-buffer content at len 16676 hash `83f08ab7e6be49a671f96f7c0ffbb71fe28bba858929b883c5904140cabd5192`, then closeout committed len 16669 hash `dc278d260b5b2a3efffeb12c428f0f81f5be1c8373fdd3ef044eee4b39adda4f`; `live_buffer_classify` suppressed the differing operator-capable buffer with `reason=write_provenance_newer_than_buffer` and no durable sidecar preserving the 16676-byte content was found. Fix the operator-wins merge/write-provenance path so concurrent operator text is preserved end-to-end or the write fails closed with a recoverable artifact. Plan: tasks/agent-doc/plan-queue-edit-race-consolidated.md
- [ ] [#tmuxsynccrashlog] [operator-verify] Replacement for #tmuxsynccrash: JB Sync Tmux Layout live repro log review. After the operator runs repeated JB Sync Tmux Layout against the repro layout, inspect idea.log, agent-doc ops.log, and tmux server pid/session evidence; close only if the server pid stays stable with repair_layout no-op/passive supersede markers, otherwise file a follow-up bug with the failing log markers. Plan: tasks/agent-doc/plan-sync-tmux-layout-crash.md
- [ ] [#60ev] plan-dynamic-context-injection-ledger.md
- [ ] [#2y8j] [operator-verify] #qbasehashmemo live typing proof: after the supervisor auto-installs 0.34.49 and recycles, type a sustained burst in a large doc (equityfundingsource.md or agent-doc-bugs2.md) in the JB editor and confirm typing is smooth; grep ops.log to confirm crdt_merge_base no longer fires per keystroke during a typing burst.
- [ ] [#cdtcutover] [focused-cycle] #agent-doc-feature FUTURE/SEPARATE rung — wire the evented CellDocTree model into the live finalize/write path (replace whole-tree reconcile at finalize with update_to per-node deltas; expose SemTree queries to preflight/supervisor drainable computations). Large, live-supervisor-critical, gated, own focused cycle + operator live proof. Plan: tasks/agent-doc/plan-celldoctree-evented-reflection.md
- [ ] [#coredecomp] [focused-cycle] Keep the former `agent-doc-core` God-crate surface decomposed into owned pure crates. Audit `pending.rs`, `queue_item_lifecycle.rs`, `template.rs`, `crdt_sync.rs`, `component.rs`, and related modules; move element-local semantics into `agent-doc-element-*`, document composition into `agent-doc-document`, merge/CRDT merge policy into `agent-doc-merge`, and FFI ABI into `agent-doc-ffi`. Do not recreate `agent-doc-core` as a compatibility facade or migration alias, and do not move runtime IO, snapshots, commits, IPC, tmux, or controller effects into pure crates.
- [ ] [#orchdecomp] [focused-cycle] Decompose the `agent-doc-orchestration` God crate along effect boundaries. Audit turn lifecycle, document realtime scheduling, queue/exchange lifecycle, closeout/commit, editor IPC, supervisor/controller state, tmux integration, recovery/repair, and session routing; move stable responsibilities into `agent-doc-turn`, `agent-doc-document`/document-realtime, `agent-doc-supervisor`/controller-state crates, `agent-doc-editor`, `agent-doc-tmux`, and narrow closeout/recovery crates as appropriate. Leave `agent-doc-orchestration` as a shrinking effect adapter only, not a deprecated compatibility facade; callers should move directly to the owning focused crate and pure merge/document code should stay out of orchestration.
- [ ] [#skillharnessplugin] [focused-cycle] Create the agent-doc integration plugin for `skill-harness`: implement an agent-doc-owned `SkillHarnessPlugin` adapter, define its detection and skill-path contract without depending on `agent-doc-core`, register/document the integration path, and add tests proving `skill-harness` can resolve agent-doc through the plugin boundary after `agent-doc-core` removal.
- [ ] [#crdtauth6] [operator-verify] CRDT authority live proof: verify the existing JB DocumentListener -> CrdtReplicaForwarder.forwardLocalDelta path, Kotlin Unix-socket ReplicaTransport, VS Code koffi replica seam, and Rust fan-out stay connected in a live editor session. This is proof/parity work only; operator-visible text is already authoritative and must not wait on this item. Plan: tasks/agent-doc/plan-crdt-authority-model.md
- [ ] [#nsfz] [operator-verify] #startupdeadline live dogfood: drive a real harness that never reaches a dispatch-ready prompt (e.g. auth-walled/hung TUI) and confirm the session fails closed within 30s with a 'session did not become dispatch-ready in Ns' stderr diagnostic + a recorded startup_miss marker, instead of hanging. Code-complete + tested (make check 5101 green, release build green); needs a live editor/pane eyeball.
- [ ] [#kj5flive] [operator-verify] #mergestatemachine3 live proof replacement for cleared review #kj5f: after the latest JB plugin update is installed, verify JB+VSCode attach/detach/ownership facts flow through the merge-ownership state machine via #lazilystatesync, then run live editor-attached/detached repros for File Cache Conflict and the #6b5h wedge with logs sufficient to prove/disprove the fix before filing follow-up bugs. Plan: tasks/agent-doc/plan-merge-control-state-machine.md
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
