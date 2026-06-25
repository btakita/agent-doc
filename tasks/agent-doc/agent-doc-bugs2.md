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
  '#spec-test-build-install-commit-push': update spec + tests. build + install for local testing. commit + push
  '#spec-test-commit-push': update spec + tests. commit + push
queue: start
---

## Status

<!-- agent:status patch=replace -->

done
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

❯ *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7a9b103bd5b68223dac05a4c449ac03899aba53d6022e82100578b5b0-20260625-153725.md`*

Compacted content:
❯ - Archived 19 response topic(s): #nsq2 JB mirror eviction on close; IPC proof diagnostic (interrupted-cycle recovery); explicit id should be used when adding a backlog item; 16 more
- Prior summary/context: ❯ *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7a9b103bd5b68223dac...

### Re: IPC proof diagnostic (interrupted-cycle recovery) — agent-doc

```text
**IPC proof issue dogfood log**
Appended automatically during interrupted-cycle recovery to record the editor IPC issue.
This is binary-authored diagnostic content, not a user prompt, so it does not require a separate response cycle.
Issue class: `ipc_proof_insufficient`
Affected component: editor IPC / writeback

ipc_proof_insufficient file=tasks/agent-doc/agent-doc-bugs2.md source=socket_ack_content patch_id=dc72f9c1-c985-413c-b01b-022503c0e251 invariant=live_prompt_drift_after_preflight recovery=content_ours_snapshot_next_cycle snap_source=ack_content_sidecar candidate_len=60065 candidate_hash=385c34aa55dd9c1bff89fd36664ffff7994ce817f7492d00ee5f6a1ccf7657fc content_ours_len=60563 content_ours_hash=900e71be19da58a63aacd1b5e188513f274cbcd0aa6ba895be0afe2bc2a35d4d doc=agent-doc-bugs2 session=b26b9957-8de6-4eae-bb98-5d7fe4d6b781 turn=cycle-1782383824251
```

### Re: #lazilycrdtspec — CRDT as the first of several multi-write mechanisms — opus-4-8

Captured, and threaded into both the plan and the backlog item. The refinement generalizes the second cell kind so it isn't welded to CRDT.

**Before:** two cell kinds — `local/direct` (single-writer) vs `replicated/CRDT` (multi-writer). CRDT *was* the multi-writer mechanism.

**After (your refinement):** the distinguishing axis stays single-writer vs **multi-write**, but *how* a multi-write cell converges becomes a **parameter of the cell**, not the kind. A multi-write cell carries a `merge: <mechanism>` attribute; **CRDT (yrs) is the first mechanism** the spec defines — picked because it converges without coordination — and the kind stays open so later mechanisms slot in alongside it:

- `merge: crdt` — yrs op-merge, the first/default
- `merge: lww` — last-writer-wins (timestamp/lamport)
- `merge: ot` — operational transform
- `merge: lease` — lease/lock-serialized single-live-writer
- `merge: fn` — application-supplied deterministic merge function

Everything downstream is unchanged regardless of mechanism: derived cells are still deterministic functions of roots (never multi-write), the merge is still an *ingress* operation on root cells only, and the cell remains the merge unit (`#qcellmerge1`). The attach/detach authority SM still only governs how many writers are *live*, not which mechanism a cell uses — the mechanism is chosen statically by what the cell represents.

**Landed this cycle:**
- `tasks/agent-doc/plan-crdt-authority-model.md` — "Two cell kinds" section now models a `multi-write` category with a pluggable `merge:<mechanism>` attribute (`crdt` first), not a hardcoded `crdt` kind (committed `033c97a75`).
- `#lazilycrdtspec` backlog item re-scoped to the pluggable-mechanism framing.

`#lazilycrdtspec` stays `[focused-cycle]` (sibling-repo `src/lazily-spec`) — the actual spec authoring drains in its own fresh-context cycle with this framing baked in.

### Re: #lazilycrdtspec — spec authored: CRDT as the first multi-write mechanism — opus-4-8

Drained the focused-cycle item. The prior cycle re-scoped the framing; this cycle **authored the spec** in the sibling repo `src/lazily-spec` (commit `6e86dea`, superproject gitlink bumped `e6f5feea7`).

**New normative chapter `cell-model.md`** (wired into the mdbook: `SUMMARY.md`, `introduction.md`; renders to `book/cell-model.html`):

- **Single axis = concurrent-writer count.** A cell is **single-writer** (`local`/`direct`, no merge) or **multi-write** (`merge: <mechanism>` ingress). Kind is static — chosen by what the cell represents, no dynamic per-write mode-switching. Locality does not make a cell multi-write; a *second concurrent writer* does (a one-way mirror like `#lazilystatesync` stays single-writer).
- **Multi-write = one category parameterized by a pluggable `merge:` mechanism — NOT a hardcoded `crdt` cell kind.** `crdt` is the **first** normative mechanism (converges without coordination); `lww` / `ot` / `lease` / `custom` are reserved extension points. Every mechanism MUST be deterministic; an unimplemented one MUST be rejected explicitly, never aliased to `crdt`.
- **Merge is an ingress op on ROOT cells only.** Derived cells are never multi-write (deterministic functions of roots → single-writer); effects stay single-writer authority (state convergence ≠ effect authorization).
- **Cell = merge unit (`#qcellmerge1`).** Each multi-write cell converges independently; a merge MUST NOT move content across cell boundaries — cross-cell contamination (the `crdt::merge` whole-document splice class) becomes impossible by construction.
- **Liveness vs mechanism.** The attach/detach authority SM governs how many writers are *live*, never a cell's kind/mechanism.

**Wire surface:** `protocol.md`'s Distributed plane reframed as `merge: crdt` (one mechanism, not *the* mechanism); `schemas/distributed.json` gains a `MergeMechanism` enum (`crdt` first) + a `CellKind` descriptor (`single_writer` | `multi_write {merge}`).

**Verified:** `distributed.json` valid JSON; `make check` (Lean formal model) green; `mdbook build` renders `cell-model.html`.

This is the foundation rung for the CRDT-authority model (`plan-crdt-authority-model.md`); the implementation siblings `#lazilycrdtrs` / `#crdtauth1–4` build on it in their own focused cycles.

### Re: #lazilycrdtspec queue head — striking (already drained) — opus-4-8

> **Queue prompt:** do [#lazilycrdtspec]: Make CRDT the first of several multi-write mechanisms.

Striking this head — the item was drained in the previous response this session: spec authored in `src/lazily-spec` (commit `6e86dea`, gitlink `e6f5feea7`, response committed `05edfe484`), `#lazilycrdtspec` backlog item reaped `[x]`. The `cell-model.md` chapter specifies single-writer vs multi-write cells with a pluggable `merge:<mechanism>` attribute, `crdt` first. No further work on this id.

### Re: #lazilycrdtrs — CRDT-backed multi-write cell kind implemented in lazily-rs — opus-4-8

> **Queue prompt:**
>
> :pushpin: do [#lazilycrdtrs]


Implemented the replicated/CRDT cell kind per `#lazilycrdtspec`, in `src/lazily-rs/src/crdt.rs` behind the existing `distributed` feature (commit `e235f98`, superproject gitlink `51417638f`).

**The binding (key design point):** a `ReplicatedCell<C>` is a multi-write **root** cell. A remote-op **ingress merge** folds a remote replica's CRDT state, then pushes the converged value into an ordinary reactive cell via `Context::set_cell` — so downstream derived slots recompute through the normal direct mechanism. The `PartialEq` guard applies *after* merge, so a losing/equal/redundant merge invalidates nothing (verified by recompute-count assertions).

**Pieces:** `MergeMechanism` enum (`Crdt` first/implemented; `Lww`/`Ot`/`Lease`/`Custom` reserved + fail-closed via `resolve()`); `Hlc`/`HlcStamp` (deterministic caller-supplied wall time); three CRDT registers `LwwRegister`/`MvRegister`/`PnCounter` (all commutative/associative/idempotent); `CellCrdt` trait; `ReplicatedCell` binding.

**Verified:** 7 new unit tests + full `make check` green (all feature suites, doctests, Lean model, benchmarks); default build (no `distributed`) unaffected. Next rungs: `#crdtauth1`/`#crdtauth2`. Plan: `tasks/agent-doc/plan-crdt-authority-model.md`.
<!-- agent:boundary:0f858373 -->
<!-- /agent:exchange -->

## Queue


<!-- agent:queue priority preset="#spec-test-build-install-commit-push" priority go -->
- ~~:pushpin: do [#lazilycrdtspec]: Make CRDT the first of several multi-write mechanisms.~~
- ~~:pushpin: do [#lazilycrdtrs]~~
- :pushpin: do [#crdtauth1]
- :pushpin: do [#crdtauth2]
- :pushpin: do [#crdtauth3]
- :pushpin: [#brtc]
- :pushpin: do [#crdtauth4]
- :pushpin: do [#xdocsuper]
- :pushpin: Fix the root cause of the CRDT churn.
- :pushpin: do [#cgfx]
- :pushpin: do [#gww8]
- :pushpin: do [#n529b]
- :pushpin: [#cgfx]
- :pushpin: [#gghx]
- :pushpin: [#8g9w]
- :pushpin: [#6mjw]
- :pushpin: [#spy9]
- :pushpin: [#r5at]
- :pushpin: [#6n5j]
---
:pushpin: JB `Run Agent Doc` on equityfundingsource.md after switching from codex to op. I have to kill + restart the tmux pane or recycle the supervisor for the opencode state to pick up. The supervisor should reactively pick up the new agent state.
```
resync: pruned 2 stale session(s)
[route] target tmux session: 0
Error: authoritative actor record for tasks/professional/equityfundingsource.md is running harness codex, but frontmatter now resolves to opencode; deferring to boundary agent restart instead of replacing live pane
```
```
[route] target tmux session: 0
Error: authoritative actor record for tasks/monsterrodholders.md is running harness codex, but frontmatter now resolves to opencode; deferring to boundary agent restart instead of replacing live pane
```
```
[route] target tmux session: 
Error: authoritative actor record for tasks/agent-doc/agent-doc-bugs2.md is running harness codex, but frontmatter now resolves to opencode; deferring to boundary agent restart instead of replacing live pane
```
---
---
:pushpin: equityfundingsource.md turn has an IPC issue:
`### Re: IPC proof diagnostic (interrupted-cycle recovery) — agent-doc`

```text
**IPC proof issue dogfood log**
Appended automatically during interrupted-cycle recovery to record the editor IPC issue.
This is binary-authored diagnostic content, not a user prompt, so it does not require a separate response cycle.
Issue class: `ipc_proof_insufficient`
Affected component: editor IPC / writeback

ipc_proof_insufficient file=/home/brian/work/btakita/agent-loop/tasks/professional/equityfundingsource.md source=file_ipc patch_id=ca5b06f9-838b-4b60-9ce2-00519736fbb1 invariant=no_ack recovery=retry_without_disk_write timeout_secs=2 patch_file=/home/brian/work/btakita/agent-loop/.agent-doc/patches/71b0db3f68b7c54571017f333b0989d1db07efa4e5ecf5ec73437e2e857072ad.json doc=equityfundingsource session=62fe1f41-886c-4f3d-b6fa-48d46106ec27 turn=cycle-1782344406484
```
---
---
:pushpin: lazily-rs.md IPC error
```text
**IPC proof issue dogfood log**
Appended automatically during interrupted-cycle recovery to record the editor IPC issue.
This is binary-authored diagnostic content, not a user prompt, so it does not require a separate response cycle.
Issue class: `ipc_proof_insufficient`
Affected component: editor IPC / writeback

ipc_proof_insufficient file=tasks/software/lazily-rs.md source=socket_ack_content patch_id=- invariant=missing_response_probe recovery=retry_without_disk_write response_sha256=8c56928442bd4b165543ab0aad264d681b41b8898551d10f196018cd15a37304 content_len=11944 content_hash=46b47fdfe5505fb37c9e63df4167cb923df6b4ffc997ecca9009a15979c29a5e doc=lazily-rs session=3d9ff8e4-bafd-4e1f-b48d-550d3c976904 turn=cycle-1782339826601
```
---
- :pushpin: [#nsfz]
- :pushpin: Still getting JB `File Cache Conflict` dialogs.
- :pushpin: Opencode responses are reverse ordering numeric lists.
- :pushpin: do [#npe1]
- :pushpin: do [#p2j4]
- :pushpin: do [#4pdp]
- :pushpin: do [#0bmv]
---
:pushpin: JB `Run Agent Doc` on equityfundingsource.md after switching from codex to op. I have to kill + restart the tmux pane or recycle the supervisor for the opencode state to pick up. The supervisor should reactively pick up the agent state change.
```
resync: pruned 2 stale session(s)
[route] target tmux session: 0
Error: authoritative actor record for tasks/professional/equityfundingsource.md is running harness codex, but frontmatter now resolves to opencode; deferring to boundary agent restart instead of replacing live pane
```
```
[route] target tmux session: 0
Error: authoritative actor record for tasks/monsterrodholders.md is running harness codex, but frontmatter now resolves to opencode; deferring to boundary agent restart instead of replacing live pane
```
```
[route] target tmux session: 
Error: authoritative actor record for tasks/agent-doc/agent-doc-bugs2.md is running harness codex, but frontmatter now resolves to opencode; deferring to boundary agent restart instead of replacing live pane
```
---
- :pushpin: do [#actorswitchdefer]
- :pushpin: do [#capproofbg]
- :pushpin: do [#nsfz]
- :pushpin: do [#gghx]
- :pushpin: do [#8g9w]
- :pushpin: do [#6mjw]
- :pushpin: do [#spy9]
- :pushpin: Are the lazily-rs contexts distinct for each running agent document? I'm seeing sessions working on other open documents.
- :pushpin: ~~Would a reactive state machine help to manage the supervisor document bindings?~~ — auto-struck: answered this cycle (#ftstrike)
- :pushpin: ~~I'm still seeing duplicate queue items added by agent-doc. Do we need to have state machine(s) represent queue activity to ensure these sort of issues don't happen?~~ — auto-struck: answered this cycle (#ftstrike)
- :pushpin: ~~I'm still seeing JB `File Cache Conflict` prompts. Do we need plugin/binary state machine(s) to control the merges to ensure disk writes don't happen while the editor/FFI plugin is running?~~ — auto-struck: answered this cycle (#ftstrike)
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
- [ ] [#crdtauth1] [focused-cycle] agent-doc authority state machine: Detached (git-authoritative, ephemeral CRDT) to EditorAttached (multi-replica) keyed on plugin attach/detach, riding the hosting-epoch backbone; extends #mergestatemachine. Plus state-vector sync protocol replacing whole-.yrs snapshot merge_by_component. Plan: tasks/agent-doc/plan-crdt-authority-model.md
- [ ] [#crdtauth2] [focused-cycle] Editor-as-replica binding (FFI-first): cdylib is the CRDT node hosting a per-editor yrs replica; thin IntelliJ/VSCode bindings forward Document deltas + apply remote updates (cursor/undo preserving); plugin read/write-through FFI (completes #dsqa). Wire agent-doc shared roots (document/queue/hosting) to lazily replicated cells (per-cell merge #qcellmerge1); upgrade #lazilystatesync to bidirectional. Plan: tasks/agent-doc/plan-crdt-authority-model.md
- [ ] [#crdtauth3] 2 [focused-cycle] State-vector commit barrier: replace finalize patch-ack with flush-all-live-editors-to-SV then snapshot then commit; root-fix for the no_ack / ipc_proof_insufficient / postcommit-worktree-corruption class. Plan: tasks/agent-doc/plan-crdt-authority-model.md
- [ ] [#crdtauth4] [focused-cycle] Multiple-editor relay + awareness + disk demotion: supervisor broadcast hub for N replicas (unique client-ids, ephemeral awareness channel, commit is a consistent cut of live replicas); .yrs demoted to write-through recovery projection, FFI nodes persist replicas locally. Plan: tasks/agent-doc/plan-crdt-authority-model.md
- [ ] [#xdocsuper] [focused-cycle] Cross-document supervisor contamination: one route-owned supervisor (per pane) reused across documents (corky.md→agent-doc-bugs2.md) carries prior doc's stale in-memory CRDT/queue/lease into the new doc → the live_prompt_drift / postcommit-worktree / answered-head-residue churn. Add a document-switch state-reset guard + SimWorld. Plan: tasks/agent-doc/plan-cross-document-supervisor-contamination.md
- [ ] [#n529b] #lazilystatesync3b Wire MirrorProjectionSummary into EditorCommandStateMachine / dispatch-ready / busy consumers (PromptPoller, RunAgentDocAttemptLedger) so they read StateProjectionBridge.mirrorSummaryForFile instead of the cold projectionSummaryForFile pull, then dogfood live in JB. Mirror substrate shipped in 0.2.190 (#n529). Plan: tasks/agent-doc/plan-lazily-plugin-state-sync.md
- [ ] [#gww8] #lazilyspecpin=Pin agent-doc state type_tag vocabulary (8 node kinds) + conformance snapshot/delta pair in lazily-spec as schemas/agent-doc-state.json (versioned) so kt/js/rs import one canonical vocabulary. Design pinned in tasks/agent-doc/plan-lazily-plugin-state-sync.md (Wire-shape definition section). Sibling-repo (src/lazily-spec) cycle per scope-discipline.
- [ ] [#cgfx] #queuestatemachine2 Route convergence through the SM; reduce dedup passes to transition guards; property test idempotence (re-emit=no-op) over #rt83qflood/#qdedupsync/#qauthorder duplication shapes. Plan: tasks/agent-doc/plan-queue-activity-state-machine.md
- [ ] [#brtc] #queuestatemachine3 SimWorld: stale-CRDT/supervisor re-emit storm converges to one item per identity; operator position-lock survives. Plan: tasks/agent-doc/plan-queue-activity-state-machine.md
- [ ] [#r5at] #lazilystatesync4 VSCode plugin: lazily-js mirror graph applying FFI state deltas. Plan: tasks/agent-doc/plan-lazily-plugin-state-sync.md
- [ ] [#6n5j] #lazilystatesync5 Parity+tests: lazily-spec cross-editor conformance (extend EditorCommandStateMachineTest to reactive path) + FFI round-trip proving kt/js mirror converges to rs authoritative graph. Plan: tasks/agent-doc/plan-lazily-plugin-state-sync.md
- [ ] [#gghx] #waitmachine1 Wait-machinery state machine core: add agent-doc-orchestration/src/wait_machine.rs with a pure WaitState/tick transition + single GLOBAL_HANG_CEILING (10s) + typed ReinstallPause exemption. Plan: tasks/agent-doc/plan-wait-machinery-state-machine.md
- [ ] [#8g9w] #waitmachine2 Migrate scattered waits (start.rs AutoTriggerMonitor 30s, route/startup.rs wait_for_agent_ready 30s, route.rs busy/shell waits, IPC ack) onto WaitMachine and lower budgets to the 10s ceiling (operator: never hang >10s). Plan: tasks/agent-doc/plan-wait-machinery-state-machine.md
- [ ] [#6mjw] #waitmachine3 Deterministic bound proof: per-state transition tests + exhaustive (state×signal) property test asserting Σdwell≤10s on every exempt-free path + SimWorld hang-class coverage. Plan: tasks/agent-doc/plan-wait-machinery-state-machine.md
- [ ] [#spy9] #waitmachine4 Lean model: formal/wait_machine/ encoding WaitState/Step mirroring Rust tick, theorem no_hang (exempt-free ⇒ dwell≤10s) + reinstall-pause sole-exemption lemma + Rust↔Lean parity test. Plan: tasks/agent-doc/plan-wait-machinery-state-machine.md
- [ ] [#nsfz] [operator-verify] #startupdeadline live dogfood: drive a real harness that never reaches a dispatch-ready prompt (e.g. auth-walled/hung TUI) and confirm the session fails closed within 30s with a 'session did not become dispatch-ready in Ns' stderr diagnostic + a recorded startup_miss marker, instead of hanging. Code-complete + tested (make check 5101 green, release build green); needs a live editor/pane eyeball.
- [ ] [#capproofbg] Make the JB Run Agent Doc capacity proof non-blocking: dispatch immediately and run the capacity proof in the background, surfacing failure asynchronously instead of stalling every dispatch.
- [ ] [#npe1] #codefencestrip=Template patch parser strips trailing code fences at the exchange boundary
- [ ] [#p2j4] #jbcfdiag=JB File Cache Conflict: investigate remaining write-path triggering the JB VFS dialog beyond IPC-first writes
- [ ] [#4pdp] #ocreverselist=OpenCode reverse-ordered numeric list rendering investigation
- [ ] [#0bmv] #qeditrace=Consolidate queue-edit-during-active-turn race plans (plan-qdurcrash + plan-supervisor-direct-queue-edit-race)
- [ ] [#actorswitchdefer] [focused-cycle] Part B: NEW SimWorld restart-flow tests (codex→opencode: assert route defers + idle-watch emits harness_change_detected→agent_restart_triggered→agent_restart_performed; paused/stuck regression asserts defer-bail recovery suffix + no silent drop) + route-side agent_change_restart-disabled explicit bail. Part A shipped (6c7455a8); SPEC.md:141 defer contract reconciled (d2312da7). Needs make tmux-ci + new restart-flow SimWorld infra. Plan: tasks/agent-doc/plan-actor-switch-defer-recovery.md
<!-- /agent:backlog -->

## Review
 
<!-- agent:review -->
- [/] [#kj5f] [operator-verify] #mergestatemachine3 remaining: JB+VSCode thin reporters emit attach/detach/ownership facts through the merge-ownership SM (via #lazilystatesync) + live editor-attached/detached repro for File Cache Conflict + #6b5h wedge. DONE: SM core Phases 1-2 + deterministic scenario repros for both wedge classes (agent-doc 34d7023d). Remaining needs the operator-paused plugin install + a live editor to verify. Plan: tasks/agent-doc/plan-merge-control-state-machine.md
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
<!-- /agent:icebox -->

## Completed / Reaped

<!-- agent:done archive=tasks/agent-doc-bugs2.done.md -->

<!-- completed work archived in tasks/agent-doc-bugs2.done.md -->
<!-- /agent:done -->
