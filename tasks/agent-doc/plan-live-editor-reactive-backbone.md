# Plan — live_editor pipeline onto the lazily reactive backbone (#live-editor-reactive)

## Status (2026-07-11) — COMPLETE (S1–S4 + S4b + S5); S6 re-scoped as the sidecar-retirement follow-up, S7 decided
S1 (reactive RelayHub liveness core, 824a2632), S2 (reactive editor open-docs registry),
S2b + S3 (resolver/visible-write route through the reactive open-docs authority, 55d91244),
**S4 (lazily `editor_open_docs` is the authority; the durable lease is crash-recovery backup
consulted only on a cold miss; sidecars/leases are off the steady-state hot path, d5d346e6)** and
now **S4b (the editor-attached gate `authority_for_file` reads pure reactive state on the hot path;
editor **crash** is observed by a cross-platform OS process-exit watcher that flips a lazily `alive`
cell; the lease is consulted only on a cold miss)** are **DONE**. The observed phantom-`live_editors=0`
wedge is fixed: after a controller recycle the doc is *untracked* in the reactive authority, so the
resolver recovers once from the durable lease and keeps editor authority instead of demoting to disk.
An explicit editor deregister (close) drives the reactive authority closed and resolves to disk
directly — no lease read. The reactive registries are driven by explicit in-process editor events
(replica register/update → open+attach, deregister → close+detach) in `agent-doc-crdt-relay-io`, so
steady-state decisions touch no filesystem. With S4b the **last** per-decision lease poll on the CRDT
op hot path is gone: `authority_for_file` no longer reads the plugin-owner lease on every op.
S5 is now DONE too (both plugins delegate to a real lazily reactive `StateGraphMirror` — JB toolchain
bumped to K2/JVM21/2024.2 to consume lazily-kt; a StateGraphMirror was ported into lazily-js for the
VS Code extension). S6 is now RE-SCOPED as the active sidecar-retirement follow-up (S5 invalidated its
rejection premises) → `plan-sidecar-retirement-lazily-sync.md`; S7 decided (keep the
content-CRDT vs state-projection split). See the staged section for per-stage detail.

## Problem

`live_editor` liveness is **imperative and polled**, so a stale editor lease strands
consumers on a phantom `live_editors=0` and both content-authority and tmux
pane/selection sync go dead while the plugin is still alive.

Observed symptom (2026-07-11): JB plugin alive (CrdtReplicaForwarder polling every
~6s, `updates=0`), yet controller logs `live_editors=0` +
`realtime_doc_resolve authority=disk reason=crdt_relay_no_live_editors` every second.
Tmux pane stopped syncing with editor document selection because focus/selection
sync has no live editor to coordinate with.

## Root cause — two unreconciled liveness representations

**Path A — imperative (drives the runtime decision):**
- `RelayHub.members: HashMap<u64, Member>`, `Member.live: bool`
  (`agent-doc-document-realtime/src/crdt_relay.rs`)
- mutated imperatively: `register()`→true (254), `disconnect()`→`m.live=false` (279),
  `reconnect()`→true (296)
- aggregated by pull-scan `live_count()` (216) + ~7 `.filter(|m| m.live)` scans
- hubs live in `static Mutex<HashMap<String, RelayHub>>`
  (`agent-doc-crdt-relay-io/src/lib.rs:104`), mutated under the registry lock
- **consumed** (pull) by:
  - `agent-doc-crdt-relay-io/lib.rs` runtime resolver → `crdt_relay_no_live_editors`
    → `authority=disk` (the every-second log)
  - `agent-doc-commit-io` → `live_buffer_guard::ensure_no_live_editor_buffer_ahead_of_disk`
  - `agent-doc-plugin-owner::live_editor_endpoint_attached`

**Path B — already reactive, different substrate (NOT read by the resolver):**
- `agent-doc-state-backbone` — lazily `ThreadSafeStateChart`, `Transport` StateDomain,
  guards `editor_synced` / `transport_no_listener` (`adstatechart.rs`)
- `crdt_authority.rs::authority_for_projection` derives `MultiReplica` vs
  `GitAuthoritative` from `DocumentStateProjection.transport.editor_generation`

The runtime reads Path A. Liveness is polled, so a stale `live` never propagates.

## Target — one reactive signal, observed end-to-end

lazily **0.21.6** (already pinned) exposes everything needed — no version bump:
- `ThreadSafeContext::cell(v) -> CellHandle<T>` (`get_cell`/`set_cell`) — reactive input
- `::computed(f)/memo(f) -> SlotHandle<T>` — derived reactive value
- `::signal(f) -> ThreadSafeSignalHandle<T>` — observable
- `CellMap`/`CellFamily<K,V>` (`cell_family.rs`) — keyed family, maps onto `members`

### Reactive topology
> **Send constraint (verified against lazily 0.21.6):** `RelayHub` lives in
> `static Mutex<HashMap<String, RelayHub>>`, so it must be `Send`. `CellMap`/`CellFamily`
> are bound to the single-threaded `Context` (`Rc`-based, `!Send`) — cannot be stored in
> the hub. Only `ThreadSafeContext` (Send+Sync) qualifies, and it has `cell`/`computed`/
> `memo`/`signal` but **no `CellMap`**. So per-member liveness cannot be a cell-per-member
> family in the hub; use a `Send`-shared liveness store + an epoch cell driving the derived
> count (no reliance on dynamic per-cell dependency tracking).

1. `RelayHub` owns: `ctx: ThreadSafeContext`, `liveness: Arc<Mutex<HashMap<u64,bool>>>`
   (the **single** per-member liveness store — `Member.live` is REMOVED), `epoch:
   CellHandle<u64>` (bumped on any liveness transition), and `live_editors: SlotHandle<usize>`
   = `ctx.computed(read epoch → lock liveness → count trues)`.
2. `register`/`disconnect`/`reconnect` mutate `liveness` (lock→mutate→**drop lock**), then
   `set_cell(epoch, epoch+1)` → `live_editors` recomputes reactively. `live_count()` becomes
   `ctx.get(&live_editors)` (reactive read, not a scan). The ~7 `.filter(|m| m.live)`
   delta-routing sites read `liveness` instead of `Member.live` (same single source).
   **Deadlock guard:** never hold the `liveness` lock across `set_cell` (the computed
   re-locks `liveness`); mutate-then-signal, lock dropped first.
3. **Bridge Path A → backbone:** a member liveness transition emits a `Transport`
   receipt into the state-backbone projection, so `authority_for_projection` (Path B)
   and the runtime resolver read ONE signal. Resolver derives authority from the
   backbone projection instead of `hub.live_count()`.
4. **Consumers observe:** resolver, `live_buffer_guard`, `plugin-owner` subscribe to
   the derived signal (via `signal`/effect) rather than re-poll.

### Can RelayHub BE a lazily `ReactiveFamily`? (verified 2026-07-11)
Conceptually perfect (members keyed by `client_id`; reactive membership + derived
count native), but **not as-is** — two independent blockers:
1. **Version:** `ReactiveFamily` is lazily **0.28.0**; agent-doc pins **0.21.6**.
2. **Send (hard blocker):** `ReactiveFamily<K,V,H>` is `Rc`-based
   (`src/lazily-rs/src/reactive_family.rs`: `inner: Rc<FamilyInner>`,
   `factory: Rc<dyn Fn>`) → `!Send`. `RelayHub` must be `Send` (static
   `Mutex<HashMap<String,RelayHub>>`). No thread-safe/`Arc` family flavor exists in
   0.28 (`thread_safe.rs` has no family; no `impl Send for ReactiveFamily`). Same
   constraint rules out 0.21.6 `CellMap`/`CellFamily`.
   Also: `Member.replica` is a yrs `ReplicaState`, not a reactive scalar — a family
   would key liveness/delivery scalars (live/generation/ack), replica stays side-stored.

Paths to a real ReactiveFamily:
- **(A) [chosen S1] hand-rolled thread-safe reactive core on 0.21.6** — `ThreadSafeContext`
  + `Arc<Mutex<LivenessMap>>` + epoch cell + derived count slot. This *is* a one-scalar
  thread-safe reactive family; ships now, no bump. Shape kept family-like for easy swap.
- **(B) add `ThreadSafeReactiveFamily` (Arc-based) to lazily-rs** (Brian owns the lib) —
  mirror how `ThreadSafeContext` mirrors `Context`; then liveness becomes a first-class
  `ReactiveFamily<u64, LivenessState>`. Cleanest end-state; lazily feature + 0.21→0.29 bump.
- **(C) make hubs thread-affine** (drop global `Mutex<HashMap>`, one hub per actor/thread) —
  then a plain `Rc` `ReactiveFamily` fits. Large runtime threading change.

### Prerequisite DONE (2026-07-11)
lazily bumped **0.21.6 → 0.29.0** across 12 agent-doc crates (commit 0a66491b) — the
`ThreadSafeReactiveFamily` (0.29.0) is now available. **Non-breaking:** whole workspace
compiles clean; core consumers pass (document-realtime 200, state-backbone 230, merge 42).
So the earlier "hand-rolled epoch cell" workaround is UNNEEDED — use the real family.

### Staged execution (each stage: functionality + SimWorld tests together, `make check`, commit)
- **S1 — reactive core in RelayHub** (using the real `ThreadSafeReactiveFamily`, not a
  hand-roll). RelayHub owns: `ctx: ThreadSafeContext`, `liveness: ThreadSafeCellFamily<u64,bool>`
  (keyed by client_id; factory `|_| true` = live-on-register), `membership_epoch: CellHandle<u64>`
  (bumped on register so the count picks up new keys), `live_editors: SlotHandle<usize>` =
  `ctx.computed(read epoch → count present_keys whose cell is true)`. `disconnect`/`deregister`
  → `set_cell(false)`; `reconnect` → `set_cell(true)`; `register` → materialize cell + bump epoch.
  `live_count()` = `ctx.get(&live_editors)` (reactive read). The ~7 `.filter(|m| m.live)`
  delta-routing sites read `liveness.observe(&ctx, id)`. Note: family is deferral-not-dealloc
  (present set only grows) — a deregistered client_id's cell stays present-but-false (not counted);
  bounded per session, acceptable. Deadlock guard: family releases its lock before touching `ctx`
  (already true in the shipped family), so no lock-order cycle with the registry mutex.
  SimWorld: liveness transitions recompute the derived count deterministically.
### Redirection (2026-07-11, operator) — editor open-files is the ground-truth reactive input
The observed wedge was "document open in the editor but `live_count=0`". Root insight from
the operator: the CRDT relay's per-replica liveness is a *derived, repairable* signal, not the
ground truth. The **ground truth is the editor's open/visible file set**, and the authority
rule is: *if there is any chance the editor buffer differs from disk, the editor buffer is
authority.* A file open in an editor can diverge from disk → editor-authoritative; a file not
open in any editor cannot diverge → disk-authoritative. So a stale CRDT lease (liveness 0 while
the file is open) must **repair liveness toward the open-doc signal**, never demote to disk.

**FFI-first (Shared Foundation).** The reactive open-doc model lives in the shared Rust library;
editor plugins stay thin event reporters (report open/close, no native reactive authority). The
reactive graph is process-local, so the plugin process is fed by FFI events and the controller
process reconciles the same open set from the durable live-buffer sidecars. Native plugin
reactive models, if any, are UI *views* of the Rust-owned truth — never a second authority.

- **S2 [revised] — reactive editor open-docs registry.** `agent-doc-document-realtime::editor_open_docs`:
  `ThreadSafeContext` + `ThreadSafeCellFamily<String, DocOpenState{open,is_agent_doc}>` +
  `membership_epoch` + derived `open_count` / `open_agent_doc_count` slots. Source-agnostic driving:
  `mark_open`/`mark_open_with`/`mark_closed` (in-process FFI events) and `reconcile(open_set)`
  (controller sidecar scan). FFI `agent_doc_report_editor_state` → `mark_open_with` (lazy
  frontmatter classify, first-open only); `agent_doc_document_closed_for_editor` → `mark_closed`
  once no live-buffer sidecar remains. SimWorld: open/close sequence vs reference model; open
  agent-doc subset; reconcile close-dropped/open-new. **DONE (this commit).**
- **S2b — authority from open-docs + liveness repair. DONE (2026-07-11, commit 55d91244).**
  Ground truth is the editor's open-file set, proved by a live-buffer sidecar with a live pid
  (`live_editor_endpoint_attached` / `#6b5h`). The resolver reconciles the reactive `editor_open_docs`
  registry from that sidecar ground truth **controller-side** (`reconcile_and_observe_editor_open`
  in `agent-doc-document-realtime-io`) — closing the process-local prerequisite below — and keeps
  editor authority when the editor is open + `live_count()==0` instead of demoting. The dropped
  replica re-registers on the editor's next edit (existing phantom-heal in
  `relay_replica_update_for_file`), so an explicit re-register in the resolver is unneeded.
  - **PREREQUISITE (verified 2026-07-11) — CLOSED, then superseded by S4.** `EditorOpenDocs` was
    fed only in the *plugin* process via FFI (`ffi.rs:597`/`1019`), so `is_open` was always false in
    the *controller*/resolver process. The first S2b pass reconciled it from the durable lease
    before *every* zero-live decision — correct, but that put the lease on the hot path. **S4
    (d5d346e6) replaced that**: the controller now drives `editor_open_docs` from explicit in-process
    editor events (replica register/update → `mark_open`, deregister → `mark_closed`, in
    `agent-doc-crdt-relay-io`), so the reactive authority is truthful controller-side with no
    per-decision filesystem read; the lease is read only on a cold miss (see S4 + the sidecar note).
  - **CPC-write demote (`crdt-relay-io` `crdt_cpc_write_disk_authority_stale_lease`, `lib.rs:1219`)
    deliberately unchanged.** With zero live replicas there is no replica to write through; forcing
    editor authority there reintroduces the documented CAS `retry_crdt_merge` strand wedge, and disk
    clobber is already prevented downstream by `disk_write_permitted_for_file`. Recorded as a scope
    decision, not an omission.
- **S3 — resolver observes reactive projection. DONE (same commit 55d91244).** Both the read
  resolve (`try_resolve_current_doc_with_disk_inner`) and the visible-write reconcile
  (`guard_visible_write_reconcile_with_target`) route the zero-live authority decision through the
  reactive `editor_open_docs().is_open` projection (pure `resolve_zero_live_editors` decision fn)
  instead of a raw `live_count()` demote. The `crdt_relay_no_live_editors` / `live_editors=` log
  fields are preserved; the repair path adds `crdt_relay_stale_lease_editor_open` /
  `recovery=keep_editor_authority_no_live_replica`. SimWorld: reactive-registry-driven transition
  test + two integration tests updated to keep-editor-authority semantics.
- **S4 — lazily state is the liveness authority; sidecars off the hot path. DONE (d5d346e6).**
  Per operator directive: *replace the sidecar with lazily state as the authority, with the sidecar
  as durable crash backup only if needed; sidecars must never be on the hot path.*
  - **Authority = reactive `editor_open_docs`**, driven by explicit in-process editor events in
    `agent-doc-crdt-relay-io`: `register_replica_for_file`/reconnect + `relay_replica_update_for_file`
    → `mark_open`; `deregister_replica_for_file` → `mark_closed`. These are explicit plugin-sent
    signals (editor attach / active / close), so the authority stays truthful with no filesystem read.
  - **Hot path reads reactive state only.** `observe_editor_open_in` (pure core in
    `agent-doc-document-realtime-io`) reads `is_open`; it consults the durable plugin-owner lease via
    the injected `lease_attached` probe **only on a cold miss** (`!is_tracked` — a doc the authority
    has never recorded, i.e. right after a controller recycle before any editor event re-seeded it),
    then caches the recovered state so later reads stay purely reactive. `is_tracked`
    (non-materializing) is the cold-miss signal.
  - **Precision gain over the first pass:** an explicit editor deregister (close) now resolves to
    disk immediately via the reactive state, instead of being overridden by a lagging live lease; the
    real symptom (recycle with the editor still open → untracked cold miss → lease recovery) keeps
    editor authority.
  - **Other consumers unchanged (no demote bug):** `live_buffer_guard`'s `commit_barrier_ready`
    routes through the controller model and **fails closed**; `plugin-owner` is the lease writer.
    Converting them to reactive subscription is a no-behavioral-change refinement, deferred.
- **S4b — the editor-attached GATE (`authority_for_file`) is now reactive on the hot path. DONE
  (2026-07-11).** The gate reads pure reactive state (`editor_attach().is_tracked/is_attached`);
  the plugin-owner lease is consulted **only on a cold miss** (a document this process never
  recorded — a CLI with no watcher, or the controller right after a recycle before any editor event
  re-seeded it). The missing crash signal is supplied by a **cross-platform OS process-exit watcher**
  (operator requirement: Linux, macOS, Windows). Shipped shape (`#s4b-liveness-cell`):
  - **Reactive topology** — `agent-doc-document-realtime/src/editor_attach.rs`: `alive:
    ThreadSafeCellFamily<pid,bool>` (the crash input), `registered: ThreadSafeCellFamily<(doc,pid),
    bool>` (driven by the replica lifecycle), and `is_attached(doc)` = *any registered (doc,pid) whose
    `alive[pid]` is true* — a pure in-memory reactive read, zero filesystem. One `process_exited(pid)`
    cascades to every doc that pid owned (whole-editor death). `is_tracked` is the cold-miss signal.
  - **OS watcher** — `agent-doc-controller-io/src/process_exit_watcher.rs`, installed once in
    `serve_with_options`. It is a **portable liveness poller** behind the injectable
    `ProcessExitWatcher` seam: `kill(pid,0)` on Unix (Linux + macOS), an `OpenProcess`/
    `WaitForSingleObject` handle wait on Windows (`windows-sys`, `cfg(windows)` target dep), a
    conservative alive fallback elsewhere. It runs on a dedicated background thread off the hot path
    and has **no per-pid OS resource** to leak (`unwatch` just drops the pid). The per-OS *event*
    primitives (`pidfd`/`kqueue`/`RegisterWaitForSingleObject`) remain a latency optimization that can
    drop in behind the same seam later without touching any consumer.
  - **CLI-safety rule** — the reactive cache is trusted only in a process that will observe a crash.
    The long-lived controller installs the watcher; a short-lived CLI does not, so `attach` is a no-op
    there, the doc is never `is_tracked`, and `authority_for_file` keeps cold-missing to the lease —
    byte-for-byte the pre-S4b behavior, and crash-safe because the lease pid-liveness is read fresh.
  - **Seeding** — `agent-doc-crdt-relay-io` drives `editor_attach().attach(doc, lease.pid)` on
    register/update (pid learned once from the lease) and `.detach(doc)` on deregister, alongside the
    existing `editor_open_docs` marks.
  - **SimWorld** — the exit-event source is injectable, so the derived authority is deterministically
    testable without real process death (fake watcher + scripted `process_exited`), including the
    whole-editor-death cascade and the reused-pid reattach. `make check` green.

  Historical rationale (why this was deferred and why it is now safe) — the gate
  `agent_doc_plugin_owner::crdt_authority::authority_for_file` → `ownership_liveness_for_file` →
  `read_plugin_owner_lease` was a filesystem lease read at the top of *every* CRDT op
  (register/deregister/update/resolve, ~30 sites), deciding `MultiReplica` (editor attached) vs
  `GitAuthoritative` (detached). It could **not** be naively reactive-cached the way S4 did the
  open-docs decision, because the reactive event stream cannot observe an editor **crash**: a
  crashed editor sends no `deregister`, so a cached `MultiReplica` would go stale and wrongly hold
  editor authority (blocking disk commits / stranding closeout). S4b closes that exact gap by making
  the crash an OS-observed reactive input (the `alive` cell) instead of relying on a per-decision
  lease poll; `plugin-owner` already depends on `document-realtime`, so the reactive read is a clean
  layering. All ~30 op sites benefit from the single `authority_for_file` change.
  - **Preferred fix — an OS process-exit EVENT, not a poll (`#s4b-pidfd`).** The missing signal is
    "editor crashed" (pid death sends no `deregister`). Get it as an event:
    - **`pidfd` (fits the current connect-per-request transport):** `pidfd_open(2)` (Linux 5.3+)
      returns a pollable fd that becomes readable the instant the target process exits (crash OR
      clean), and works across process trees (the editor is not the controller's child, so
      `SIGCHLD`/`waitpid` don't apply — `pidfd` does). The controller opens one pidfd per attached
      editor (pid learned once at register/attach — an event, not a per-decision poll), registers it
      on the existing accept-loop epoll (`rpc.rs:5782`), and on the exit event calls
      `editor_open_docs.mark_closed`. macOS equivalent: `kqueue` `EVFILT_PROC`/`NOTE_EXIT`.
    - **Persistent-connection socket `HUP` (cleaner, but a transport change):** today the JB
      transport does `SocketChannel.open(...).use{}` — a fresh connection per ~6s poll, so there is
      no long-lived fd to watch. If the editor held ONE persistent Unix-domain-socket connection, the
      controller's epoll `POLLHUP`/EOF would be the crash/disconnect event and the connection would
      *be* the liveness lease (no pid bookkeeping, no lease file for liveness at all). Bigger change;
      the better long-term shape.
    With `pidfd`/`HUP` events + the existing `deregister` events both driving `mark_closed`, the gate
    can read pure reactive state on the hot path and read the lease **only on a cold miss**.
  - **Reactive topology (`#s4b-liveness-cell`) — the OS event feeds a lazily source cell; authority
    is a DERIVED slot.** Do not treat the pidfd/HUP exit as an imperative `mark_closed` callback;
    treat it as a **source-cell write** so process liveness becomes a first-class node ON the reactive
    graph and authority derives reactively (the S1 `RelayHub` liveness pattern lifted to this layer):
    - `alive: ThreadSafeCellFamily<pid, bool>` — the reactive INPUT. `pidfd_open(pid)` exit event →
      `set_cell(alive[pid], false)`; register / cold-miss lease seed → `true`.
    - `registered: CellFamily<(doc, pid), bool>` — driven by `replica_register` / `deregister`.
    - `is_open(doc)` / authority = `computed(...)` — a derived slot: *any registered `(doc, pid)`
      whose `alive[pid]` is true*. `authority_for_file` reads this slot: pure in-memory reactive read,
      zero filesystem.
    Two payoffs: (1) **whole-editor death cascades reactively** — one `set_cell(alive[pid], false)`
    recomputes every doc that pid owned to closed (the derived slot fans out); per-doc close stays the
    `deregister` cell write; the lease drops to a pure cold-miss seed. (2) **the crash-detection
    blocker dissolves** — the crash is just another reactive input, so the gate can read the derived
    slot with no lease poll. The OS-event source stays SimWorld-injectable (feed synthetic
    `set_cell(alive[pid], false)`), so the derived authority is deterministically testable without
    real process death — which a `kill(pid,0)` poll never could be.
  - **Per-doc close vs whole-editor death:** `pidfd`/`HUP` detect *whole-editor* death; a graceful
    per-doc close (editor stays alive, closes one buffer) is covered by the explicit `deregister`
    event. Together they replace the lease poll entirely, leaving the lease as pure cold-miss/durable
    backup — the operator's model. (A graceful lease *release* with no `deregister` and no exit is the
    only residual; it should not occur if close always sends `deregister`.)
  - **Test note:** a pid-only `kill(pid,0)` cache is insufficient AND untestable in-process (the
    test's "owner pid" is the test process, which never dies). A `pidfd`/`HUP` event source is
    injectable in a SimWorld (feed synthetic exit events), so the gate conversion stays deterministically
    testable. This remains real work across the gate's ~30 call sites; deferred to its own stage, not
    shipped unsafely. Until then the lease poll at `authority_for_file` is a deliberate backstop, and
    the S4 directive holds for the open-docs *liveness decision* but not yet for the *editor-attached
    gate*.

## Missing reactive components in the lazily plugins (#plugin-reactive-core)

Audit (2026-07-11): the editor plugins are **thin reactive mirrors**, not lazily
reactive-core participants, and they do **not** ride lazily's `Bridge`. This is
correct for the FFI-first "Shared Foundation" intent (Rust owns the authoritative
graph; plugins are views) but leaves three concrete reactive components unbuilt.
Each is a candidate stage; none is a regression, all are parity gaps.

**Where reactive state lives today**
- **Authority (reactive):** lazily-rs `ThreadSafeStateChart` in `agent-doc-state-backbone`
  — the single source of truth. ✅
- **Wire:** `agent-doc-state-wire` emits lazily-spec-shaped `snapshot`/`delta`
  (`cell_set`/`slot_value`/`invalidate`/…) over agent-doc's own Unix-socket IPC + FFI
  (`agent_doc_state_subscribe` → controller `state_subscribe` RPC). ✅ but bespoke.
- **Plugin mirrors (NOT reactive-core):** JB `StateGraphMirror.kt` is a gson hand-fold
  that cannot import lazily-kt (`#lzpkgwire`: IntelliJ Kotlin 1.9/JBR17 vs lazily-kt
  Kotlin 2/JVM21); VSCode `stateMirror.ts` is a bespoke adapter because `@lazily/js`
  ships only the FFI consumer + IPC helpers, no reactive core. Both are conformance-pinned
  1:1 to the lazily reference but are *deterministic folds*, not live reactive graphs.
- **Document content (NOT the reactive graph):** a separate yrs CRDT path
  (`CrdtReplicaForwarder`/`CrdtReplicaManager` + `.agent-doc/patches/<hash>.json`).

**Missing components (decisions recorded 2026-07-11)**
- **S5 — real lazily reactive-core mirror in each plugin. DONE (2026-07-11).** Both plugins now
  delegate the snapshot/delta state graph to a real lazily reactive-core `StateGraphMirror` instead
  of a hand-rolled fold. The two `#lzpkgwire` blockers were resolved, not deferred:
  - **JB / lazily-kt:** the earlier framing (needs a shaded artifact for IntelliJ 1.9/JBR17) is
    superseded — `#lzpkgwire` was two version gaps (JVM 21 bytecode + Kotlin 2.0 metadata), and the
    real fix was to **bump the plugin toolchain** to Kotlin 2.0 / JVM 21 / IntelliJ **2024.2+**
    (`sinceBuild 242`; drops 2024.1). lazily-kt uses no JDK-21-only APIs, so no lazily-kt change was
    needed. The plugin's `StateGraphMirror` is now a thin adapter delegating to
    `io.github.lazily.StateGraphMirror` (Gson parses the wire → constructs lazily-kt's public
    `WireSnapshot`/`WireDelta` → `inner.applySnapshot`/`applyDelta`; `decodeSubscribe` stays
    `internal`). Wired via a **conditional Gradle composite build** (`includeBuild` the sibling
    lazily-kt when present, else `mavenLocal`/GitHub Packages — standalone-safe). Only
    `lazily-kt-<v>.jar` is bundled; every platform-provided transitive (kotlinx-coroutines —
    **must not** be bundled — kotlin-stdlib, kotlinx-serialization, annotations, jna) is excluded.
    `StateGraphMirrorConformanceTest` + `StateProjectionBridgeTest` stay green.
  - **VSCode / lazily-js:** the earlier framing (`@lazily/js` ships no reactive core) is now stale —
    lazily-js **does** export the reactive primitives. It shipped no `StateGraphMirror` *class*,
    though, so S5 **ported one into lazily-js** (`src/state-graph-mirror.js`, a real reactive graph on
    `Context`: per-node payload cells + a memoized derived `summary()` slot; export
    `@lazily-hub/lazily-js/state-graph-mirror`; conformance test against the shared lazily-spec
    fixtures). The extension's `StateGraphMirror` now delegates to it, loaded via esbuild bundling so
    the packaged `.vsix` is self-contained (the ESM lazily-js is inlined into the CJS extension
    bundle). `MirrorTurnProjection` (no lazily equivalent) is still computed plugin-side from the
    delegated closeout.cycle cell. `stateMirror.test.ts` / `stateMirrorConformance.test.ts` stay
    green. The conformance pins are kept as cross-language drift catches.
- **S6 — retire the sidecars onto lazily reliable sync. RE-SCOPED (2026-07-11): now the active
  north-star follow-up → [`plan-sidecar-retirement-lazily-sync.md`](plan-sidecar-retirement-lazily-sync.md).
  PROGRESS (2026-07-11): Phase 0 (lazily-spec reliable-sync protocol, f18d7e9), Phase 1
  (lazily-formal `ReliableSync.lean`, 1749fbd), and Phase 2 impl across all three bindings
  (lazily-rs 3200cb3, lazily-kt 67c980f, lazily-js 81116e5) are DONE — `ResyncCoordinator` /
  `DurableOutbox` / OR-set+LWW liveness, control frames as `IpcMessage` variants (msgpack
  cross-lang codec). The lazily-spec `coverage.json` "Reliable sync" row is already ✅ for rs/kt/js,
  and the deferred rs `SyncDriver` loop skeleton is now DONE too (lazily-rs `reliable_sync.rs`,
  behind `feature="ipc"`, 8 SimWorld tests); the `webrtc`-ungate proved unnecessary (the pull/driver
  path rides `IpcSink`/`IpcSource`, not `BridgeHub`). Remaining: Phase 3 agent-doc integration (3A
  UDS carrier, 3B pull-on-lazily-types, 3C plugin push on the CrdtSync plane, dual-run cutover).**
  The earlier "reject, keep the bespoke wire" rested on premises S5 invalidated and one that was
  simply wrong: (1) "single-step lazily-IPC deltas" — false, `lazily::Delta` is `{base_epoch, epoch,
  ops: Vec<DeltaOp>}`, already an epoch-batched delta set; (2) the CommonJS/ESM + IntelliJ-toolchain
  blockers — resolved by S5 (JB on K2/JVM21; a `StateGraphMirror` ported into lazily-js); (3) "a
  bridge without a plugin-side reactive graph buys nothing" — S5 landed that graph. The remaining
  substantive facts still shape the design, not a rejection: agent-doc's epoch is a cumulative
  *accepted-event count* (a delta may span epochs), and lazily's Bridge is fire-and-forget (no
  retry/outbox/ack, `ResyncRequired` unwired, no per-doc channel, `webrtc`-gated). So the plan is
  **not** "adopt `BridgeHub`" — it is: push the *reliable-sync protocol* (resync coordinator, durable
  outbox, sync-driver, OR-set liveness) into lazily (spec+formal→rs/kt/js), keep agent-doc's
  pull+epoch+SQLite for controller→consumer, and move the plugin→controller open-set/lease push onto
  the idempotent CrdtSync plane — retiring the live-buffer + plugin-owner-lease sidecars (the
  stale/phantom/divergent-read race class). Full design + phasing in the linked plan.
  **Wire codec (decided 2026-07-11):** the cross-language plugin(kt/js)⇄controller(rs) sync stream
  negotiates lazily's `msgpack` codec (self-describing, evolution-safe, portable), not JSON;
  `postcard` stays the Rust-only fast path and capnproto/protobuf were rejected (IDL+codegen cost
  across 8 bindings, opaque payloads waste their typed edge). Formalized in `lazily-spec/protocol.md`
  § Frame codecs; pinned across all three `IpcMessage` variants in `lazily-rs/tests/ipc.rs`.
**Can the durable filesystem sidecars be replaced by lazily state? (analysis 2026-07-11)**
The editor-liveness truth crosses **two OS processes** (plugin ⇄ controller). Today the durable
carriers are filesystem sidecars: the **plugin-owner lease** (`live_editor_endpoint_attached`,
pid-live — what S2b reconciles from) and the **`.agent-doc/live-buffer/` sidecars** (`#lbreap`
per-editor buffer markers). These are not competing authorities with lazily state — they are the
**durable, cross-process persistence+transport layer** *under* the reactive projection:
`sidecar/lease (durable cross-process truth) → reconcile → editor_open_docs (reactive projection)
→ resolver decision`. lazily `editor_open_docs` is the in-process reactive *view*, not a second
source of truth. Replacing the sidecars with lazily state needs three things that don't exist yet:
1. **Cross-process reactive transport** — lazily graphs are process-local; carrying an open-set
   *cell* from the plugin process to the controller process is exactly lazily's `Bridge` (S6,
   currently rejected — and a bridge buys nothing until the plugins host a real reactive core, S5,
   blocked by `#lzpkgwire`).
2. **Durability across a controller recycle** — the phantom-0 scenario *is* a controller restart
   that loses in-memory state; the sidecar/lease survive on disk, which is why they can't be
   dropped. lazily state would need a durable projection (or re-derivation from the socket
   handshake on restart).
3. **Origin ownership in the plugin's reactive graph** — S5's real plugin reactive core so the
   open-set cell is authored reactively rather than written as a lease file.

**What S4 delivered against this (2026-07-11, d5d346e6):** within the *controller* process, lazily
`editor_open_docs` is now the **authority** for the liveness decision and the durable lease is
**off the hot path** — consulted only on a cold miss (post-recycle recovery), exactly the operator
directive. This did *not* need S5/S6 because the controller is fed by explicit in-process editor
events (the replica lifecycle over the existing socket), not by a cross-process reactive bridge.
What still needs S5 + S6 + a durability story is the *fuller* north star — **eliminating the durable
filesystem lease/sidecar entirely** (a pure lazily open-set cell authored in the plugin's reactive
graph and bridged to the controller, with lazily-owned durability across a recycle). Until then the
lease stays as the crash-recovery backup only; it is no longer a per-decision authority read.

- **S7 — content CRDT vs reactive projection. DECISION: keep split (two substrates, one socket).**
  The yrs content replica (`CrdtReplicaForwarder`/`CrdtReplicaManager` + `.agent-doc/patches`) and
  the lazily state projection (`state_subscribe` snapshot/delta) serve different jobs — conflict-free
  *text* convergence vs *UI-state* projection — and yrs is the right tool for the former. They are
  deliberately not unified; `#live-editor-reactive` liveness (now observed end-to-end at the resolver
  via S2b/S3) does not require merging the transports. Revisit only if a single reactive transport
  becomes a concrete requirement.

### Test conventions (per operator feedback)
- SimWorld over mocks for sync/tmux; deterministic model + shared pure decision fn.
- Do NOT touch the fixed-seed fuzz. Split functionality + tests in the same change.

## Immediate recovery (separate from the migration)
The live session's pane sync can be restored now by healing the stale lease
(re-register / reconnect the plugin replica so `live_count()` returns to 1) — track
independently of the reactive migration.
