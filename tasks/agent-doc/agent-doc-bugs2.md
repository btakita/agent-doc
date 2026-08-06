---
agent_doc_session: b26b9957-8de6-4eae-bb98-5d7fe4d6b781
resume: 019fabf0-9af0-7f73-a25c-6a43fd6470c5
agent: opencode
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
  '#code-review': Please review agent-doc codebase. Inspect the architecture and look for edge cases. Isolate concurrency issues and other workflow bugs. Let's find all contributing factors to fix.
  '#commit-push': commit + push all uncommitted files
  '#follow-up-backlog': Any follow-up items to place in the backlog?
  '#mermaid': Include explanatory mermaid diagram(s).
  '#next-steps': Any follow-up items to place in the backlog? Please ensure the backlog is in priority order. Print the item id of the top of the backlog.
  '#simworld': Strengthen Simworld tests.
  '#spec-test-build-install-commit-push': red-green-refactor. update spec + tests. build + install for local testing. commit + push
  '#spec-test-commit-push': red-green-refactor. update spec + tests. commit + push
queue: go
---

## Status

<!-- agent:status patch=replace -->

completed
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7a9b103bd5b68223dac05a4c449ac03899aba53d6022e82100578b5b0-20260806-032502.md`*

Compacted content:
- Archived 2 response topic(s): Agent Doc recovery and root-cause fixes; Response
- Prior summary/context: prior compacted content: Archived 30 response topic(s): #pcc2 and the haiven-dev closeout wedge; #pcc3; do [#pcc4]; 2...

### Re: tmux auto-sync delay — reactive investigation + first conversion shipped — glm-5.2

> **Queue prompt:**
>
> do [#jetbrainsrunroute-tgp5]
>
> do [#jetbrainsrunroute-1ypa]
>
> do [#jetbrainsrunroute-8x66]


Investigated the full editor tab-switch → tmux pane-focus latency path. The
reactive plane itself is exemplary (`agent-doc-editor-surface/{lib,graph}`,
`idle_revision.rs`, `idle_watch.rs`) — no `#stategraphjoin` islands, single
state-machine cells, derived `Computed`s, gated `Effect`s, and `#idlerevisionreactive`
is fully landed (no residual 500ms supervisor poll). The wedges live in the
**imperative I/O scaffolding** around that plane, ranked by impact:

1. **`controller_tmux_layout_for_surface`** (`rpc.rs:17198`, called under the
   editor-observe mutex at `rpc.rs:16985`) — for every ≥2-column editor
   observation it synchronously surveys tmux (per-pane `pane_session`
   `display-message` spawns, `list_panes_ordered`, per-pane process-owner
   resolution), recomputing a structural mirror the controller already derives
   reactively (`runtime.pane_layout_desired()`). This is both the primary latency
   wedge and an imperative-state violation (`#reactive-boundary-ingress`).
2. **Editor 500ms fixed-delay poll** (`TmuxPaneFocusSync.kt`) polling
   `tmuxFocusState` per root every 500ms — state the controller already publishes
   on the editor-surface projection state plane (`rpc.rs:17004`).
3. **Sync lock sleep-poll** (`sync-io/lib.rs:138`) — 50ms granularity vs a 100ms
   SafePassive budget; contention triggers a full `/proc` reap.
4. **Per-pane `process_tree_contains_pid` outside the observation scope** in the
   sync ownership loop.

**Shipped in 0.35.145 (this cycle):** the `sync --no-autostart` run now opens a
`ProcessObservationScope`, so the per-document ownership loop reads pane owners
through the reactive `owner_graph` (`#syncownerreactive`) instead of re-walking
`/proc` for every candidate pane on every pass. Each pane root PID is observed
once and its derived owner cells are reused for the run; tmux layout mutations
don't invalidate process trees, so the `LocalReadScope` stays valid. This is the
contained, high-confidence reactive conversion + a real sync/claim latency win.

**Remaining conversions captured** as backlog `#tmuxautosyncreactive` (top of
list) with plan `tasks/agent-doc/plan-tmux-auto-sync-reactive.md`: item 1 is the
headline (fold editor observations against a retained tmux observation fed by the
existing `pane_layout_effect_worker`, with a contained `pane_session`
snapshot-batching sub-fix), plus items 2–4. These are genuinely larger hot-path
changes to the controller with drift-detection tests to reconcile, so they get a
focused cycle rather than a rushed mid-investigation landing.

**Cleanup:** marked the three stale queue heads `#jetbrainsrunroute-tgp5`,
`-1ypa`, `-8x66` done — all are duplicate reports of the
`accepted_without_dispatch_start_proof` failure already resolved by
`#jetbrainsrunroute-77k5` (same document, same symptom-key invariant).

**Deploy:** `make check` green (incl. clippy + TLA), installed, submodule pushed
+ tagged `v0.35.145` (CI in progress — observed, not awaited), supervisor
recycled onto the fresh binary at its idle boundary. The agent-loop **superproject**
push was deferred: `main` has a large pre-existing divergence (~160 local commits
vs 10 remote, across many session submodule-pointer commits) that hit a submodule
merge conflict on rebase — that history reconciliation is a pre-existing condition
for the operator, not something to force-rewrite; the release itself ships from the
submodule and is out.
<!-- agent:boundary:2463ea63:agent-doc-bugs2 -->
<!-- /agent:exchange -->
## Queue

<!-- agent:queue priority preset="#spec-test-build-install-commit-push" go -->
- do [#tmuxautosyncreactive]
- ~~do [#jetbrainsrunroute-tgp5]~~
- ~~do [#jetbrainsrunroute-1ypa]~~
- ~~do [#jetbrainsrunroute-8x66]~~
- There is a significant delay with tmux auto-sync when switching editor agent-doc documents. Please optimize all bottlenecks and fix any wedges.
<!-- /agent:queue -->

## Notes

<!-- agent:notes -->
<!-- /agent:notes -->

## Backlog

<!-- agent:backlog priority queue -->
- [ ] [#tmuxautosyncreactive] #agent-doc-bug Significant delay with tmux auto-sync when switching editor agent-doc documents: convert the remaining imperative state operations to Lazily reactives. PRIMARY wedge = controller_tmux_layout_for_surface (rpc.rs:17198) does a synchronous per-observation tmux probe under the editor-observe mutex, recomputing state the controller already derives reactively (runtime.pane_layout_desired). Also: editor 500ms fixed poll (TmuxPaneFocusSync.kt) polling state already published on the state plane; sync lock sleep-poll (sync-io/lib.rs:138); ControllerEditorSurfaceGraph Mutex<BTreeMap> -> ThreadSafeDependencyMap. 0.35.145 shipped the contained ProcessObservationScope sync fix. Plan: tasks/agent-doc/plan-tmux-auto-sync-reactive.md
<!-- /agent:backlog -->

## Review
 
<!-- agent:review -->
- [/] [#operatorverifyresolve] [operator-verify] Resolve the current JetBrains File Cache Conflict with Cancel / Keep memory changes, then save or make one editor mutation so the plugin publishes the current editor cut; confirm the retained closeout commits and a later Compact Exchange no longer fails on the historical stranded duplicate heading.
- [/] [#operatorverifyrestart] [operator-verify] Restart a running JetBrains IDE to activate plugin 0.2.340, then move from fpe-service-topology.md in a one-pane surface to agent-doc-bugs2.md in its two-column surface and confirm the first selection restores two tmux panes without a second navigation.
- [/] [#rebootselfhealverify] [operator-verify] Live proof for #rebootselfheal after restarting the IDE onto JetBrains plugin 0.2.339 (reload-lib swaps the cdylib but NOT the Kotlin package, so a restart is required). Reboot the host (or stop the controller and leave a stale .agent-doc/controller.sock behind), then run Sync Tmux Layout and Run Agent Doc from the IDE and confirm both succeed without the 'pane-layout state projection publish failed ... Connection refused' notification and without a human deleting the socket. Then confirm the passive lane is still free: click between editor tabs with NO controller running and verify no controller is launched (editors/SPEC.md requires the surface-observation lane to never launch one) - grep ops.log for a controller bootstrap around the tab clicks. Both reboot states are already proven end-to-end against the installed cdylib; only the in-IDE path needs a human eyeball.
- [/] [#hvw0] [operator-verify] Rotate the CARGO_REGISTRY_TOKEN secret on github.com/btakita/skill-harness — the current one returns 403 authentication failed, so CI cannot publish to crates.io (v0.1.3 was published manually from the passage token btak/CARGO_REGISTRY_TOKEN). Needs a repo-secret write, which is operator-gated. Until it is rotated, every tagged release will now FAIL loudly at the publish step (that is the intended new behavior after 32836ad; it previously reported success)
- [/] [#c8tb] [operator-verify] Live #jbroutebusystall proof (0b49105e1): with the supervisor socket down (route logs `route_dispatch_only_authoritative_degraded_direct_pane supervisor_health=no_socket`) and the pane mid-turn, click JB Run Agent Doc. Expect `route_start` -> `route_success` in the attempt ledger to drop from ~17s to under ~2s, with `route_existing_pane_busy_active_turn_skip_wait` in ops.log and the SAME outcome as before (`route_existing_pane_already_running`, pane focused, no duplicate injection). Unit tests pin the cue gating but cannot prove the live latency win.
- [/] [#operatorverifylive] [operator-verify] #relaylockpoison live proof: with the JB plugin loaded, force a replica panic (the crash-2026-07-09 shape: agent_doc_replica_apply_local) and confirm later replica calls still succeed instead of returning -1/null for the life of the IDE. This is the behavior the whole migration exists to guarantee and it cannot be proven without a host IDE.
- [/] [#mtds] [operator-verify] #panedraftunblocker live proof: on a Claude pane whose footer reads `⏵⏵ bypass permissions on · 1 shell · ← for agents` (the variant without `(shift+tab to cycle)`), with an EMPTY composer and no active turn, trigger JB Run Agent Doc. Expect dispatch to succeed instead of refusing with `timed_out` / `unblocker=wait_for_dispatch_ready_prompt`. Then repeat with unsent text typed in the composer and expect the new refusal naming the draft with `unblocker=submit_or_clear_pane_draft`, and NO injection over that text. Finally confirm a genuinely mid-turn pane (live spinner, bare `❯`) still refuses as busy. Fixed in 17848a927, installed 0.35.6; needs the supervisor idle-boundary recycle to pick up the fresh binary.
- [/] [#falsetyping-guard] #agent-doc-bug provenance-aware visible-write guard. INTEGRATED to main (merge on 5bcfc07b) + binary/cdylib installed + recycled; guard tests 7/7 green; JB 0.2.205 zip + VS Code 0.2.38 compiled. REMAINING (operator): install JB plugin zip (build/distributions/agent-doc-jetbrains-0.2.205.zip) + reload IDE, install VS Code 0.2.38, then live proof — type during replica churn (no wedge) + genuine unsaved edit still fails closed. NOTE: bugs2.md can still wedge until the IDE plugin is reloaded (loaded plugin still calls FFI v2 → conservative fail-closed).
<!-- /agent:review -->

<!-- agent:icebox -->
- [ ] [#agentsignals] #agent-doc-feature [future/separate] Add a realtime agent:signals element after core realtime convergence is stable. Start with deterministic built-in signals for tests, queue, supervisor, and ipc; design custom user-defined signals as plugin-backed tracked work with Rust as the first target and future multi-language support; use tasks/software/realtime-signals-patchboard.md as the GUI/semi-programmatic reference; keep MLP enhancement optional/experimental and not load-bearing for deterministic states.
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
