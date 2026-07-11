# Plan — live_editor pipeline onto the lazily reactive backbone (#live-editor-reactive)

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
- **S2b — authority from open-docs + liveness repair.** Derive per-doc authority from
  `is_open(doc)` (editor-authoritative when open, disk when not) and reconcile it with the
  backbone Transport projection so `authority_for_projection` agrees. When `is_open(doc)` but the
  relay `live_count()==0`, **repair** (re-register/reconnect the replica) instead of demoting —
  this is the detach/attach mapping the earlier S2 question was really about.
- **S3 — resolver observes backbone.** `agent-doc-crdt-relay-io` resolver derives
  authority from the reactive projection (open-docs + backbone), not a raw `live_count()` poll.
  Preserve the `crdt_relay_no_live_editors` / `live_editors=` log fields (regression-visible).
- **S4 — push to remaining consumers.** `live_buffer_guard`, `plugin-owner` observe
  the signal. Reconcile stale-lease heal so a live-but-lease-expired plugin re-lives
  reactively (kills the phantom-0 wedge that caused the symptom).

### Test conventions (per operator feedback)
- SimWorld over mocks for sync/tmux; deterministic model + shared pure decision fn.
- Do NOT touch the fixed-seed fuzz. Split functionality + tests in the same change.

## Immediate recovery (separate from the migration)
The live session's pane sync can be restored now by healing the stale lease
(re-register / reconnect the plugin replica so `live_count()` returns to 1) — track
independently of the reactive migration.
