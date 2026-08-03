# Plan — CRDT scramble fix + disk→CP-replica→editor-buffer propagation with reconcile

**Status:** design captured 2026-07-03 (operator-directed). Consolidates and sequences
three prior plans against a fresh subsystem audit. Supersedes nothing — it is the
ordering/dependency spine tying them together.

## Pattern: pid-liveness checks without identity verification (2026-07-03)

Three live-environment bugs this session share one root cause — a check that a pid is *alive*
(`/proc/{pid}` exists or heartbeat fresh) without verifying it is the *expected process*, so a
**reused pid** (a dead component's pid reassigned to an unrelated process, e.g. a kernel thread
with an empty cmdline) reads as live:

1. **Plugin-owner lease** — JetBrains never acquired one at all (below); once acquired, the Rust
   side already verifies pid liveness, but the JB frontend wasn't calling acquire.
2. **Watch daemon stale `watch.pid`** — `is_running()` was `read_pid().is_some_and(pid_alive)`;
   pid 17 (`[rcu_...]` kernel thread) had reused a dead daemon's pid, so `ensure_running` believed a
   daemon was live and refused to start one, and `--status` reported a phantom daemon. **Fix:**
   `pid_is_agent_doc_watch(pid)` verifies `/proc/{pid}/cmdline` is `agent-doc … watch` (a kernel
   thread's cmdline is empty → rejected), mirroring the supervisor's `supervisor_pid_matches_doc`.
   `is_running()` now uses it. Test + full watch-io suite green; deployed.

Lesson: liveness ≠ identity. Any pid-file / heartbeat gate must verify process identity (cmdline),
not just `/proc/{pid}` existence.

## Root cause: JetBrains never acquired the plugin-owner lease (2026-07-03)

**Why the editor-attached goals (1/4/5) never engaged in a JetBrains editor.** The JB plugin
*declared* `agent_doc_plugin_owner_try_acquire` in `NativeLib.kt` but **never called it** — only
the VS Code frontend acquires the lease (in `ownsDocument()` on patch handling). So the JB plugin
never registered a live plugin-owner lease; `authority_for_file` read every document as **headless
(not editor-attached)**, so the whole CRDT/realtime/turn-state path (replica register, C1b
propagation, turn projection) stayed dormant. After an IDE restart the leftover lease kept a **dead
pid** — the "stale lease despite restarting IDEA" symptom. **Fix (`TypingTracker.kt`):**
`refreshPluginOwner()` calls the acquire FFI with the live pid on **document open**
(`scheduleOpenDocumentReport` — re-establishes after restart), heartbeats it on each **debounced
buffer report** (`reportFullContentNow`), and **releases** on close (`clearOpenDocumentReport`).
Rust acquire takes over a dead-pid lease (defers only to a lease that is *fresh AND live*), so a
restart cleanly reclaims ownership. `compileKotlin` clean; plugin zip built. Install + reopen a doc
→ the lease shows the live IDEA pid and the document reads editor-attached, unblocking live 1/4/5.

## Landed foundations (safe, pure, tested — no live-path wiring, no install/push)

Bricks landed this session that new phases build on. Each is pure logic with unit tests +
clippy clean; none is wired into the live path yet, so live sessions are unaffected.

- **Goal 1 — turn-state CP→plugin projection + retained-stream parity (LANDED; FFI polling retired).**
  `agent-doc-turn/src/cp_projection.rs`: `TurnProjection::from_phase(CyclePhase)` → coarse
  `TurnState` (Idle/AwaitingResponse/Persisting) + `turn_in_flight` + `transition_authority`.
  `would_collide_with_in_flight_response()` is the `live_prompt_drift` double-append guard;
  `transition_authority()` encodes the single-authority invariant. The original
  unary FFI projection has since been retired: both frontends now retain
  `document_turn_authority_stream` and receive the same typed projection frame.
  **Plugin UI consumption — LANDED (2026-07-03), reactive migration 2026-08-02:**
  VS Code `buildTurnStatePresentation` (consumes the projection → status label + double-append
  forwarding guard; 2 unit tests, 19 sessionUi tests green, tsc clean) **wired live** — `extension.ts`
  `refreshTurnStatus` reflects pushed stream frames in a status-bar item. JetBrains parity:
  `TurnStateBridge.kt` (same
  projection→presentation logic, mirrors the working `StateProjectionBridge` FFI pattern).
  Remaining: JB status-*widget* wiring + a `make bump-plugin` / `npm compile` + editor reload to
  observe live.
- **Goal 4/5 — file-watch consumer decision.** `decide_watch_action` in `watch_authority.rs`
  (see Phase C0). 6 tests.
- **Goal 2 — ALREADY IMPLEMENTED (verified 2026-07-03).** The stale-binary/cdylib recycle chain
  ships end-to-end: detection `classify_stale_install_artifacts` (`supervisor/config.rs:138`,
  mtime-based, build-before-commit aware) surfaced by `stale_install_warning`
  (`orchestration/preflight.rs:594`) + `stale_supervisor_warning_for_doc`
  (`project_controller/rpc.rs:1049`); policy `supervisor_recycle_action(stale, …)`
  (`supervisor/lifecycle.rs`); execution via `recycle_inflight` + `RECYCLE_INFLIGHT_AUTO_INSTALL`
  → `RecycleImmediate`/`RecycleDebounced`/`EscalateKillRelaunch`, stale-supervisor pause clearing,
  and cdylib lib-install auto-recycle + mtime hot-reload. **No new code needed — a
  `should_recycle` brick would duplicate `supervisor_recycle_action`.** The vocabulary markers
  (`STALE_SUPERVISOR_MARKER`, `pane_title_for_status`) live in `turn_status.rs`.

## Goal (operator, verbatim intent)

1. Fix the causes of CRDT scrambling.
2. When the document file on disk changes, the file watcher updates the CP replica
   document and propagates the change through to the plugin's editor buffer.
3. If the editor buffer already has the change, reconcile it (no double-apply, no
   scramble, no lost operator text).

## Why the naive ordering is unsound

The three sub-goals look independent but share **one prerequisite**: the correct
per-replica state-vector CRDT (`agent-doc-merge/src/crdt_sync.rs` +
`agent-doc-document-realtime/src/replica_sync.rs`) is fully implemented and tested
but **dormant** — not wired to any call site. Production still runs a per-cycle
*rebuild-from-text* merge (`crdt.rs` `merge_inner` / `merge_by_component`) whose every
isolation-layer failure falls back to a whole-buffer yrs+Myers merge. That fallback is
the root of the scramble class. Building disk→buffer propagation on top of it inherits
all of it.

Two required components do not exist today and must be built for goal (2)/(3):

- **A supervisor IPC path from the watcher daemon to the canonical `RelayHub`.** The
  watcher is a separate process; `RelayHub` is process-global in the supervisor. A disk
  change currently records `FileWatchChangeObserved` into SQLite and dead-ends — nothing
  consumes it. Only editor-originated `ReplicaUpdate` reaches canonical today.
- **An enqueue-to-editor primitive on canonical change.** `reconcile_canonical_against_baseline`
  reseeds hub mirrors but never `enqueue_delivery`s — corrections never reach the live buffer.

## As-found architecture (audit 2026-07-03)

- **CRDT lib:** `yrs` 0.27, modeled as one whole-document `Y.Text`, re-segmented per merge.
  Not a durable per-cell CRDT. `crdt_sync.rs` (state-vector, idempotent causal-buffered
  `apply_update`) and `replica_sync.rs` (lazily `CrdtPlaneRuntime` HLC + `StampFrontier`
  delta sync) are the correct substrate — **dormant**.
- **CP replica:** `RelayHub` (`crdt_relay.rs:123`), one per doc, in a process-global
  registry (`crdt-relay-io/src/lib.rs:75`) in the supervisor. Allocated only under
  `CrdtAuthority::MultiReplica` (live editor attached).
- **Watcher:** `notify`-backed `agent-doc watch` daemon (`agent-doc-watch-io/src/daemon.rs`),
  separate process. On settled `.md` change → `route_file_change` (`src/main.rs:751`) →
  `FileWatchChangeObserved` StateFact → SQLite. No consumer.
- **Editor write channels:** (a) replica delta channel (editor⇄CP⇄peers via `ReplicaUpdate`/
  `ReplicaPull`/`ReplicaProjection`, forwarder polls ~4×/s); (b) patch-file channel
  (`.agent-doc/patches/<hash>.json`, `PatchWatcher.kt`). Neither is triggered by disk change.
- **Authority order:** live editor buffer > in-memory canonical > disk > git baseline.
  `reconcile_current_doc` (`read_authority.rs:69`) is the per-cycle picker. Plugin's own
  file watcher is deliberately read-only (`watch_authority.rs:203`).
- **"Already has it" detection:** content-hash/len equality (`live_buffer_diverges_from_content`,
  `write.rs:2064-2159`). Blind to pure deletions / within-line shrink (`crdt_merge_base.rs`).
- **The modeling state machine is dead code:** `DiskDriftObserved` / `PluginlessDiskSave` /
  `OperatorSourcesReconciled` in `document-realtime/src/lib.rs` — unwired.

## Phases (dependency order)

### Phase A — Point-bug guards in the fallback path (independent, ship first)

These exist in the whole-buffer fallback that stays reachable throughout the migration.
Fix regardless of the substrate work; each is unit-testable and low-risk.

> **Verification pass 2026-07-03:** the audit's Phase-A "quick wins" did not survive
> scrutiny. A1 is a phantom (see below). A2 is a narrow edge, not the flagship drop, and
> the real `#fmdrop` incident was already point-fixed off this path. **Lesson: the genuine
> remaining work is the substrate migration (B/C/D), not a Phase-A patch sweep.** Each item
> below must be reproduced with a failing test before any fix — do not batch-"fix" from the
> audit list.

- **A1 — `apply_ops` non-ASCII offset — VERIFIED NOT A BUG (2026-07-03).** The audit flagged
  `apply_ops` (`crdt.rs:1759/1763`) feeding byte offsets into yrs on a `Doc` with no `OffsetKind`
  set, assuming yrs defaults to UTF-16 (like Yjs/JS). **It does not.** yrs 0.27.2 `Options` default
  is `OffsetKind::Bytes` (`~/.cargo/.../yrs-0.27.2/src/doc.rs:560,573`), i.e. UTF-8 byte offsets —
  consistent with the module's stated byte-offset convention (`crdt.rs:1681-1683`). `compute_edit_ops`
  works in whole lines (`similar::TextDiff::from_lines`), so offsets always land on char boundaries.
  No mis-index. **Do not "fix" this — configuring UTF-16 or ASCII-gating `apply_ops` would introduce
  a real offset bug.** Lesson: verify each audit finding against the yrs default before acting.
- **A2 — frontmatter on the op hot path — NARROW EDGE, reproduce before fixing.** The op-routed
  cell path deliberately does not cell-merge frontmatter (`cell_doc.rs:349`); an op landing in
  frontmatter framing bails `route_ops_to_cells` to the whole-doc `merge_inner` yrs merge
  (`cell_doc.rs:1458`), and otherwise framing is copied verbatim from one side (`cell_doc.rs:1603`).
  So the only exposure is **concurrent frontmatter edit AND captured editor ops in the same cycle** —
  a real but narrow edge, not the flagship drop. The `#fmdrop` incident memory refers to was in the
  **snapshot/commit** path and was already point-fixed. **Action:** write a failing 3-way test
  (operator edits a frontmatter key while agent ops are captured) first; only fix if it actually
  drops/clobbers. Subsumed cleanly by B (frontmatter as its own per-cell node under state-vector merge).
- **A3 — `guard_committed_responses` wholesale revert (`crdt.rs:1561`) — reproduce before fixing.**
  The revert-to-`ours_text` arm *can* drop legitimate operator additions in theory. Not yet shown
  to fire in a real case. **Action:** construct a failing case (theirs adds new content on the same
  cycle the revert triggers) before touching the heuristic; otherwise defer to B, which replaces the
  heuristic with a state-vector check.

> **FIXED (2026-07-03) — framing-safe whole-component fallback.** Root cause pinpointed:
> `merge_aligned_nodes` (`crdt.rs`), when `reconcile_component` declines (body-only vs keyed,
> unsplittable, ambiguous keys), ran the flat leaf `merge` on the *framed* component text (markers
> included) → the text CRDT kept BOTH `<!-- /agent:exchange -->` close markers → malformed doc →
> re-segment safety net → whole-doc `merge` fallback (a *worse* scramble that also dropped `Q1.`).
> **Fix:** the whole-component fallback now leaf-merges ONLY the body between the markers and
> reframes with operator-authoritative markers, so the framing is valid by construction and
> content stays inside the component. `tests/phaseb_scramble_repro.rs` now PASSES by default
> (permanent regression guard); merge suite 209 + document-realtime 196 green; clippy clean. This
> is the near-term scramble fix (fallback layer). The deeper consolidation (retire `merge_3way` /
> whole-doc `merge` for the state-vector model) remains the Phase-B end state below.
>
> **Case 1 (structural divergence) also fixed (2026-07-03).** When component *sets* differ,
> `merge_by_component` now models it as component add/remove ops relative to base
> (`merge_divergent_component_sets`): a clean superset (component *added*, none removed/reordered,
> unique names) aligns by name-union — shared components merge per-component (framing-safe), added
> components are kept — reassembled from the superset node stream so framing is valid by
> construction. Component *deletes* are now also handled framing-safe + CONSERVATIVELY: a component
> present in base + one side is KEPT with valid framing (never dropped — the #queue-clear-unrun-items
> / append-only-exchange data-loss class), so the delete case no longer scrambles; honoring a
> legitimate operator delete is the deferred crdt_sync semantic. Only genuine reorder /
> both-added-different / malformed framing still defer to the whole-doc fallback. Tests:
> `divergent_component_sets_merge_by_union_not_whole_doc_splice`
> + `structural_divergence_delete_is_framing_safe_and_conservative`; merge 211 +
> document-realtime 196 green. Also landed: `MergeClassification` (Clean / Corrupted{reason,
> repairable}) + `merge_contents_crdt_classified` (directive: corrupt docs get a special
> designation, marker/frontmatter corruption flagged repairable), and `parse_for_startup` /
> `repair_frontmatter_yaml` (directive: malformed frontmatter must repair or surface a user-facing
> message, never a silent supervisor no-open). **WIRED + LIVE-DEMONSTRATED (2026-07-03):**
> `run::repair_document_frontmatter_on_disk(file)` runs at the *entry* of the run pipeline
> (`run_with_context`) and the start path (`start/run.rs`) — BEFORE any diff/session-ensure/config
> parse — basic-repairing malformed YAML on disk (tabs→spaces, stray-fence strip) so the supervisor
> opens; an unrepairable block leaves the clear downstream user-facing message. First wiring attempt
> was too late (an earlier parse in the run pipeline pre-empted it); moving it to the single pipeline
> entry fixed it. Live proof via the fresh binary: a tab-indented doc printed `repaired malformed
> frontmatter ... before startup`, converted the tabs on disk (`tabs_after: 0`), and the run
> proceeded past the parse (session UUID injected) instead of the cryptic `invalid YAML` failure.
>
> **Original reproduction:** A REAL production-path
> scramble is now captured in `tests/phaseb_scramble_repro.rs` (`#[ignore]`d until the fix lands,
> so it does not red `make check`; run with `--ignored` to see it fail). Concurrent agent+operator
> disjoint edits to the same `exchange` component make `merge_contents_crdt` →
> `merge_by_component` fall back to the whole-doc yrs merge (audit #5), which splices across the
> component boundary → a **duplicate `<!-- /agent:exchange -->` marker with operator content
> orphaned outside the component**. Production's `normalize_template_structure_or_fail` then
> *rejects* this mixed-content duplicate (fail-closed safety net) — so today the cycle fails rather
> than commits corruption, but the merge is wrong. **Fix target:** keep the same-component content
> divergence inside `merge_by_component`'s per-cell path instead of collapsing to the whole-doc
> merge. This test is that fix's acceptance gate.

### Phase B — Wire the per-replica state-vector CRDT into the hot path (the core fix)

This is `plan-realtime-reconcile-replicas.md` Phase 2 + the `#crdtsvdom` corrective layer of
`plan-editor-sync-barrier.md`. It is the single change that subsumes the scramble class.

- **B1 — Convergence = per-cell state, not text bytes.** Replace the heuristic base-repair
  (`merge_inner` <50% override at `crdt.rs:259`, prefix-snap at `:285`) and the text-equality
  convergence predicate with `cell_doc`/`crdt_sync` state-vector dominance. "In sync" = neither
  replica holds ops the other lacks. This kills scramble mechanisms #1, #2, #5, #6 (fallback
  collapse) and makes #9 a causal check.
- **B2 — One reconcile step, one stage.** Collapse the write-stage proof and commit-stage
  `try_auto_recover_live_prompt_drift` into a single `reconcile_turn_against_live_buffer`
  in the realtime model. Per-cell 3-way merge; loop-to-fixpoint on the live buffer with a
  **bounded** settle (reuse `await_idle_via_file`, ~500ms/2s). On timeout → fail open to the
  buffer, retain response pending. **Never a held lock** (the `no_ack` trap).
- **B3 — exchange ours-wins content-loss (`cell_doc.rs:907`).** Once per-cell state is the
  authority, an operator edit inside a `### Re:` block must merge (or fail closed to the buffer),
  not silently resolve to the agent copy. Scramble mechanism #7.
- **Guardrail:** keep yrs as persistence until the lazily plane serialization is proven; migrate
  `.yrs` → op-log snapshot in a later, reversible cutover (not in this change).

### Phase C — Disk change → CP replica (the missing propagation, needs B)

- **C0 — decision layer — LANDED (2026-07-03, tested).** `decide_watch_action(delivery,
  authority, editor_edit_in_flight) -> WatchAction` in `watch_authority.rs` is the pure consumer
  decision the audit found missing: `WatchDelivery::Change` → `ApplyAsDiskAuthority` (Detached) /
  `ReconcileIntoCanonical` (EditorAttached, settled) / `DeferForEditSettle` (EditorAttached,
  operator edit in flight — bounded, fail-open, never a held lock); non-change deliveries →
  `None`. 6 new unit tests, clippy clean. Encodes the watcher-scoping rule (watcher stays on; a
  live editor changes only the *destination*). Not yet wired — C1 calls it.
- **C1a — host seam — LANDED (2026-07-03, tested).** `apply_disk_change_for_file(file, on_disk)`
  in `agent-doc-crdt-relay-io` is the in-process entry the controller watcher calls: authority-gated
  (`GitAuthoritative` → `None`, headless owns disk), fail-open editor-sync barrier, then
  `with_hub_seeded_from_file(...).apply_disk_change`. Mirrors `reconcile_disk_projection_for_file`.
  2 tests (headless-None, editor-attached no-op). This connects `RelayHub::apply_disk_change` (D0)
  to the process-global hub registry — the Rust vertical `decide_watch_action` → this →
  `apply_disk_change` is now complete and tested.
- **C1b — cross-process call site — LANDED (2026-07-03, marker approach, tested).** Topology
  verified: the only `notify` watcher is the separate `agent-doc watch` daemon; the `hub_registry`
  lives in the supervisor. Rather than a fragile socket IPC (the repo's #1 wedge source), C1b uses
  the codebase's robust cross-process signal — a **file marker polled by the supervisor idle loop**,
  mirroring `recycle_request`:
  - **Marker path:** `agent_doc_fs::disk_change_request_path_for` → `.agent-doc/disk-change-requests/<hash>.json`.
  - **Producer** (`crdt-relay-io::route_disk_change_signal`, called from the watch daemon's
    `route_file_change` in `src/main.rs`): runs `decide_watch_action`; drops a marker only for
    editor-attached docs (`ReconcileIntoCanonical`/`DeferForEditSettle`), none for headless
    (`ApplyAsDiskAuthority`) or non-changes.
  - **Consumer** (`crdt-relay-io::consume_disk_change_reconcile`, called from the supervisor idle
    loop in `idle_watch.rs`): re-reads current disk, calls `apply_disk_change_for_file`, clears the
    marker (once, even on a headless no-op).
  - Tests: marker lifecycle + gate (crdt-relay-io 19), full touched-crate suites green
    (fs 40, supervisor 246, document-realtime 196, turn 252), orchestration wiring modules 47.
    Whole Rust vertical `daemon watch → marker → idle-loop consume → apply_disk_change_for_file →
    apply_disk_change` compiles + tests green; no install/push. The socket `IpcMethod::DiskChange`
    was prototyped then reverted — the marker is the robust, session-resolution-free path.
  - **End-to-end integration test — LANDED (2026-07-03).** `tests/c1b_disk_change_propagation.rs`
    drives the whole vertical in-process against a *genuinely editor-attached* doc (real
    plugin-owner lease held by the test PID + a registered hub replica — not mocked authority):
    additive change → idempotent `AlreadyReconciled` no-op (goal 5); out-of-band deletion →
    `RebuiltFromDisk { live_members>=1 }` (goals 4/5); headless → no marker. 3 tests green.
  - **Built + installed (2026-07-03):** `cargo build --release` + `cargo install` (binary carries
    C1b) + `lib-install` (cdylib carries the FFI export; reload broadcast to plugins). `nm` confirms
    the then-current turn-projection ABI shipped; it is now replaced by the
    controller-owned retained stream.
  - **Remaining:** the editor-buffer side of a `RebuiltFromDisk` deletion still needs D2
    (replace-capable delivery); the additive path (`AlreadyReconciled`/additive changes) already
    reaches editors via the existing `ReplicaPull` channel.
- **C2 — New supervisor IPC method** (`supervisor_io.rs`, analogous to `handle_replica_update`):
  `apply_disk_update` — integrate disk text into the canonical `RelayHub` as an external-origin
  update, using B's state-vector merge so it is reconciled, not clobbered. Under
  `GitAuthoritative`/headless (no hub) this is a no-op (disk is already authority).
- **C3 — Idempotent by construction.** Because B integrates via state-vector merge, a disk change
  the canonical already holds is a structural no-op (goal 3, the "already has it" case at the
  CP layer). Reuse `SelfWriteEcho` (`watch_authority.rs:132`) to drop the agent's own writeback.

### Phase D — CP replica → editor buffer (needs C)

- **D0 — hub disk-change entry point — LANDED (2026-07-03, tested).**
  `RelayHub::apply_disk_change(on_disk) -> DiskChangeOutcome` in `crdt_relay.rs` composes the
  existing in-memory-wins reconcile: `AlreadyReconciled` (canonical already has it — goal 5 no-op,
  idempotent), `RebuiltFromDisk { live_members }` (out-of-band correction; canonical rebuilt, and
  the live-editor count that still needs a replace-capable re-bootstrap is **reported, not silently
  dropped**), `BaselineDeferred` (no commit baseline yet). 4 tests; full crate suite 196 green.
  This is the C1 IPC hop's in-process target. **Known boundary it exposes:** an additive yrs delta
  cannot express an out-of-band *deletion*, so `RebuiltFromDisk` cannot reach the editor buffer
  until D2 (a bootstrap/replace delivery the editor applies by replacing, not CRDT-merging) — a
  Kotlin/protocol change, correctly gated.
- **D1a — re-bootstrap tracking primitive — LANDED (2026-07-03, tested).** `RelayHub` now tracks
  `pending_rebootstrap: HashSet<u64>`: `apply_disk_change` on `RebuiltFromDisk` flags every live
  editor; `pending_rebootstrap_members()` / `rebootstrap_text()` (the corrected canonical) /
  `clear_rebootstrap(id)` expose it for the delivery layer. This is the supervisor-side of D2 — the
  hub knows *which* editors need a replace and *what* text. Test + full document-realtime suite (197)
  green. **D1b — delivery host fn — LANDED (tested):** `crdt-relay-io::pull_rebootstrap_for_file`
  (authority-gated; returns the corrected canonical text for a flagged editor + clears the flag;
  logs `crdt_rebootstrap_pull`). Remaining: fold the re-bootstrap into `handle_replica_pull`'s
  `ReplicaPull` response (the editor already polls that channel), and the plugin buffer-*replace*
  apply on receipt (Kotlin + VS Code parity) — both verifiable only against a live editor.
- **D1 — enqueue-on-canonical-change primitive in `RelayHub`.** When `apply_disk_update` (or
  `reconcile_canonical_against_baseline`) mutates canonical, `enqueue_delivery` the resulting
  delta to every live member mirror. Today reconcile reseeds mirrors but never enqueues — this
  is the gap that keeps corrections out of the live buffer.
- **D2 — Editor pulls + reconciles before apply.** The existing `ReplicaPull`/`CrdtReplicaForwarder`
  path already applies remote deltas via Document API. The plugin shadow reconciles against its
  own buffer before applying (goal 3, the "already has it" case at the editor layer): if the
  buffer already carries the delta (state-vector dominance), the pull is a no-op ack; if it
  diverges, per-cell merge, operator text wins same-node. A `RebuiltFromDisk` deletion needs a
  **replace-capable bootstrap delivery** (the editor replaces its buffer rather than CRDT-merging).
  **VS Code parity (operator-required 2026-07-03):** the replace-capable delivery and
  `TurnProjection` consumption must land in BOTH the JetBrains and VS Code frontends identically —
  logic in the shared FFI, each plugin a thin consumer. Spec: `specs/14-realtime-workflow.md`
  § Editor Parity Requirement. Divergence between frontends is a forbidden shape.
- **D3 — Wire the dead state machine.** Drive `DiskDriftObserved` / `PluginlessDiskSave` /
`OperatorSourcesReconciled` (`document-realtime/src/lib.rs`) from C/D so the flow is modeled
and observable via `admin inspect`, not implicit.
- **D4 — Logical editor replica generations — LANDED (2026-07-16).** A JetBrains
`:<path>:refresh-N` identity is a new native incarnation of the same visible editor, not a
collaborative head. A per-document registration fence serializes simultaneous refresh attempts and
publishes the successor identity before hub membership changes, so a late old update cannot exploit
the register/metadata transition window. Registration retires every prior incarnation sharing the
stable logical identity, rotates and durably checkpoints the CRDT lineage, and returns the canonical
bootstrap to the sole successor. Late direct updates from a retired raw identity are terminal
no-ops; late durable document-op frames carry the retired lineage and are quarantined by the same
lineage fence. An old deregister cannot remove the replacement.

Already-corrupted two-generation projections recover before the generic integrity gate only when a
durable pending intent proves one entire branch byte-for-byte. That target is semantically rebased
over the other operator branch, preserving unsaved prompt/queue edits while materializing the agent
response once; ambiguous shapes remain blocked. Together these changes close the observed 93 KB →
179 KB whole-document duplication/second-boundary failure without electing disk over the operator
buffer.

### Phase E — Regression surface

Property-style tests injecting: (i) a disk `git checkout HEAD` while a live editor holds newer
buffer; (ii) an operator direct-edit to the `.md` while a cycle is mid-flight; (iii) a disk change
the editor already applied (idempotent no-op); (iv) sequential and concurrent logical editor
refreshes with late direct and durable frames from prior incarnations; (v) strict recovery of an
already-concatenated pending-target/operator pair; (vi) each Phase-A point bug. Assert: converges or
fails open to buffer — never corrupts HEAD, never wedges, never drops operator text. SimWorld
coverage for the disk→replica→buffer loop and exhaustive refresh-generation interleavings.

## Guardrails (do not regress)

- **Fail open to the editor buffer, never fail closed to a held lock or stale-snapshot discard.**
  Every barrier is bounded + fail-open (`plan-editor-sync-barrier.md` trap).
- **Operator-visible text is authoritative** — never overwrite with `content_ours`/snapshot/ACK
  if it would drop operator edits.
- **Sidecars are backup, not hot-path authority** (`plan-sidecar-authority-hot-path.md`) — a stale
  sidecar informs, never drives a merge; regenerate from (live doc + capture ledger) in background.
- **Never recompile the live merge path while a session is actively dropping keystrokes.** Build in
  focused cycles against a quiesced session; `make install` + `admin recycle` only at a clean
  boundary. (Stale-binary/cdylib recycle is its own goal-line — a stale supervisor running the old
  merge silently defeats this work.)

## Dependency graph

```
A (point guards, independent) ─┐
                               ├─> ship anytime
B (state-vector hot path) ─────┴─> required by C
C (disk → canonical IPC) ──────────> required by D
D (canonical → editor enqueue) ────> required for goal 2/3 end-to-end
E (regression) ────────────────────> after each phase, red-first where possible
```

## Related plans

- `plan-realtime-reconcile-replicas.md` — Phase 2/3 = this plan's Phase B/D (per-cell model + reconcile loop + FFI shadow).
- `plan-editor-sync-barrier.md` — Phase 0/1/`#crdtsvdom` = this plan's B2 bounded fail-open + state-vector dominance.
- `plan-sidecar-authority-hot-path.md` — the sidecar-is-backup invariant this plan preserves.
</content>
</invoke>
