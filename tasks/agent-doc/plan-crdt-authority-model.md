# Plan: CRDT authority model — editor-as-replica, commit barrier, FFI node

`#crdtauthority`

## Operator ask

A design conversation (2026-06-25) on the agent-loop `agent-doc-bugs2.md` session,
arising from the recurring stale-CRDT / answered-head-residue / `File Cache
Conflict` / `ipc_proof_insufficient no_ack` bug class. Questions raised, in order:

1. Should the CRDT be in-memory with a disk projection? Is the disk projection necessary?
2. Will an ephemeral/git-authoritative CRDT (Option A) support real-time editing?
3. How is the **editor buffer** factored in — it's yet another source to merge
   against — and how is **propagation lag** from the editor to the CRDT
   authority reconciled?
4. What about **multiple editors**?
5. Can the **FFI plugin be a CRDT node**, or must each editor plugin be its own
   CRDT node?
6. Would this mean lazily supports two modes of communication — direct and CRDT?

This plan is the answer, as a focused-cycle spec. It supersedes the loose
"Option A vs Option B" framing from the thread.

## Root-cause context (already fixed this session — the first rungs)

The stale-CRDT class comes from **two sources of truth that diverge**: the
committed git document and the persisted CRDT state (`.agent-doc/crdt/<hash>.yrs`).
Confirmed mechanics:

- The long-lived controller (`project_controller.rs::ControllerMemoryState`)
  holds only `actor_store` — there is **no in-memory CRDT** today. The CRDT is
  **disk-only**, loaded/merged/written by short-lived per-cycle processes.
- The live residue bug: the persisted `.yrs` **base** carries a head struck,
  while a live side (regenerated `content_ours`) carries it un-struck; the Yrs
  leaf merge reads the un-strike as the winning edit → the answered head
  reappears every cycle. Operator recovery `reset --from-current` works precisely
  because it discards the persisted CRDT.

Landed on agent-doc `main` this session:
- `2f704263` `#xdocsuper0` — claim guard consults the document supervisor lease
  (closes "two supervisors own one document").
- `2a00da6e` `#queuestatemachine2`/`#qheadresidue` — CRDT queue convergence routed
  through a per-item `Live < Struck` lifecycle lattice join so a stale CRDT
  contribution cannot resurrect a struck head (`agent-doc-core/src/crdt.rs` +
  `queue_item_lifecycle.rs`).
- `66ac6c64` `#xdocsuper1`/`#xdocsuper3` — per-document hosting-epoch backbone
  transition (`state_backbone.rs`) that makes a stale-overlay replay a no-op.
  **This becomes load-bearing under the model below** (per-document isolation of
  live in-memory replicas).

These are necessary first rungs but do not change the underlying model. The model
below is the structural fix.

## The model: one logical document, N CRDT replicas, sync — not merge

The conceptual error is treating the document as "several texts to 3-way merge"
(snapshot base, agent response, editor buffer, persisted CRDT). The fix is to
treat it as **one logical document with N replicas** (editor(s), agent/
supervisor) that exchange **ops**, not text. You stop asking "merge these N
texts" and ask "sync these N replicas." CRDT (yrs, already the lib) guarantees
the replicas converge regardless of order or lag.

### Authority follows the live editor (attach/detach state machine)

The disk projection is **not intrinsically necessary as a source of truth** — it
is necessary only as a *durable projection*. Authority is a state, transitioned
by editor attach/detach (this IS `#mergestatemachine`, the merge-ownership SM,
fed by the plugin's existing attach/detach/ownership facts):

- **Detached** (no live editor — pure CLI / Codex / headless): no concurrent
  human writer. Git is authoritative; the CRDT is **ephemeral**, rebuilt from the
  committed doc + that turn's ops. No durable `.yrs` to go stale. Cheapest, zero
  stale-state class. This is most headless dogfooding traffic.
- **EditorAttached**: a live multi-replica CRDT session. The supervisor hosts the
  canonical replica; each editor hosts its own replica; ops sync continuously.
  Git + disk are **projections checkpointed at boundaries**, never the live
  coordination medium.
- The `attach` / `detach` event is the transition — exactly what the hosting-epoch
  substrate (`66ac6c64`) models. Per-document isolation is mandatory here: a live
  in-memory replica per document is the cross-document contamination surface, so
  `#xdocsuper1/3` is a hard requirement, not a nicety.

### The editor buffer is a replica, not a source to merge (Q3)

Do not merge the editor buffer against the authority — make the **editor buffer
its own CRDT replica** and sync it. A "thin client that ships keystrokes and
renders what it is told" is the **OT** model (central server transforms ops by
revision); under OT, lag requires active rebasing and stale renders are real.
That is the fragile path. The CRDT path instead:

1. **Local-first apply.** The editor applies the user's keystroke to its *local*
   replica immediately — zero perceived lag, never blocks on the authority. Same
   for the agent's stream ops on its replica.
2. **State-vector sync, not text merge.** Each replica tracks a `StateVector`;
   sync exchanges only the *missing* updates (`encode_state_as_update(their_sv)`).
   This is the Yjs/yrs sync protocol — yrs already provides the primitives.
3. **Commutativity + causal buffering absorb the lag.** A late / out-of-order
   update merges regardless of arrival order; yrs buffers an update whose causal
   deps have not arrived and applies it when they do. **Lag = transient visual
   divergence that self-heals; never a permanent conflict, never data loss.** No
   transform/rebase step exists — ops carry their own causal context.

So propagation lag is not "reconciled" by a resolution algorithm; it is made
irrelevant by construction. (This also fixes the known data-loss case where "a
compact's smaller disk file was missing sections the live editor buffer still
held" — those ops are durable in the editor replica and sync over.)

### The one place lag has teeth: the commit barrier

Eventual consistency is fine *during* editing, but **git commit / `finalize` is a
snapshot of a moment**. Un-propagated editor ops at that instant would commit a
state missing the user's last keystrokes. That single point needs a **sync
barrier**: before snapshotting to git, the authority must confirm it holds each
live editor's updates up to that editor's *current* state vector ("flush and ack
at SV=N").

This is what today's `finalize` IPC-ack gropes at — and the
`ipc_proof_insufficient` / `no_ack` / `retry_without_disk_write` failures
littering `agent-doc-bugs2.md` are **that barrier breaking under a fragile
patch-ack scheme**. Replace patch-acks with **state-vector acks** ("editor synced
through SV=N") so the commit is provably complete instead of hoping a patch
round-tripped. Today's flakiest seam becomes a well-defined quiescence point.

### Payoff: write-ownership dissolves

If every writer is a full replica and ops commute, **no writer needs exclusive
write ownership** for correctness — agent and human can both always write and
always converge. `#mergestatemachine` collapses from "who may write now" to "who
owns the durable checkpoint and when is the barrier" — the same barrier above.
The `File Cache Conflict` dialog class disappears: the plugin never sees an
"externally modified" file because it applies CRDT updates to its bound replica
rather than reconciling text on disk.

## Multiple editors (Q4)

Multiple editor instances (two IntelliJ windows, IntelliJ + VS Code, or two
people) are simply **multiple replicas** — the model does not change for N > 1:

- **Star topology** via the supervisor as hub: each editor replica syncs with the
  supervisor's canonical replica; the hub broadcasts each replica's updates to the
  others (standard y-websocket-server shape). Peer-to-peer is possible but the
  star matches the existing supervisor architecture and the durability/commit
  story.
- **Unique stable client-ids.** yrs requires a unique client-id per replica for op
  attribution and deterministic tiebreak. Each editor process must mint a stable
  unique id (collision = corruption). The supervisor and agent are also ids.
- **Awareness / presence** (other users' cursors/selections) is a separate
  *ephemeral* channel (Yjs "awareness" protocol) — not part of the document CRDT,
  not persisted, not committed. Optional, additive.
- **Commit barrier with N editors.** At finalize the authority flushes all
  *currently-live, connected* editors to their SVs and snapshots. It must **not**
  block forever on a slow/disconnected editor: a commit is a *checkpoint*, not a
  global lock. An editor offline during a commit contributes its ops at next
  sync, yielding a new state the next checkpoint captures. So a commit captures a
  consistent cut of the live replicas; late joiners/reconnects converge
  afterward by construction.

## FFI plugin as CRDT node — yes; one impl, one node per editor process (Q5)

**The FFI library (cdylib, `ffi.rs`) is the CRDT node. Editor plugins do NOT each
reimplement a CRDT.** This is the repo's own Shared Foundation / FFI-first
principle (AGENTS.md): business logic in the shared FFI library; plugins are thin
event reporters.

- The yrs replica, sync protocol, state-vector logic, op encode/decode, and
  binding lifecycle live **once** in Rust, shared by the binary/supervisor and the
  cdylib (`crdt.rs` is already shared between them). The "CRDT node" is one Rust
  module instantiated everywhere.
- The cdylib is loaded **in-process to each editor**, so there is naturally **one
  FFI-hosted replica per editor process** (two separate IDEs = two cdylib
  instances = two replicas — exactly what you want). Each editor's FFI node syncs
  with the supervisor's node over the existing socket IPC.
- Each editor **plugin** stays thin: translate local `Document` deltas → FFI
  `apply_local_update`, and apply remote updates from an FFI callback back into the
  `Document` (preserving cursor/undo) — the IntelliJ/VS Code analog of
  y-prosemirror / y-codemirror. No CRDT logic in Kotlin/TS.
- **Why not a native CRDT per plugin?** yrs and Yjs are wire-compatible (yrs is
  the Rust port), so VS Code *could* embed Yjs directly — but that is two CRDT
  implementations that must agree byte-for-byte on update format and tiebreak;
  divergence = subtle convergence bugs. The FFI-node approach uses one impl
  everywhere → convergence is guaranteed and matches the FFI-first rule. Reserve a
  native plugin CRDT only for an editor that genuinely cannot call the cdylib.

So: **one CRDT implementation (yrs in Rust), hosted by the FFI node in each editor
process and by the supervisor; plugins are thin bindings; the supervisor is node +
relay hub + git-checkpoint authority.**

## lazily ⨉ CRDT layering — CRDT-backed cells, lazily derives on top (`#lazilycrdt`) (Q6)

CRDT and lazily are complementary layers, not alternatives. **lazily** is the
reactive derivation graph (a root cell changes → derived cells/effects recompute,
deterministically, single-writer within a runtime). **CRDT** is convergence of
concurrent writes from multiple replicas. The marriage: **shared root cells are
CRDT-backed; lazily derives reactively on top.** A cell whose value is a CRDT
register/map/text receives merged remote ops as ordinary cell updates, and lazily
propagates them downstream like any local change.

### Two cell *kinds*, one reactivity core — NOT two reactivity engines

"Does lazily then have two modes of communication, direct and CRDT?" — precisely:
**two cell *kinds*, distinguished by concurrent-writer count, not two propagation
engines.**

- **Local / direct cells** — exactly one writer (this runtime's derivation graph,
  or a single owner with a one-way delta mirror like today's `#lazilystatesync`
  Rust→Kotlin projection). Propagation is direct reactive push/pull. No merge.
- **Multi-write cells** — potentially many concurrent writers across replicas.
  New values arrive by **merging remote ops through a pluggable merge mechanism**;
  the merge result is fed into the graph as an ordinary cell update.

**CRDT is the *first* multi-write merge mechanism, not the only one (operator
refinement 2026-06-25).** The distinguishing cell axis is single-writer vs
multi-writer; *how* a multi-write cell converges is a parameter of the cell, not
baked into the kind. CRDT (yrs) is the first mechanism the spec defines — chosen
because it converges without coordination — but the cell kind must keep the merge
strategy open so later mechanisms can slot in alongside it: last-writer-wins
(timestamp/lamport), operational transform, lease/lock-serialized single-live-writer,
or an application-supplied deterministic merge function. The spec therefore models a
`multi-write` cell category with a `merge: <mechanism>` attribute (`crdt` first),
**not** a hardcoded `crdt` cell kind. Downstream reactive propagation and the
cell-as-merge-unit boundary are identical regardless of which mechanism a given
multi-write cell selects.

Crucially, **downstream reactive propagation is always the same direct mechanism.**
CRDT is an *ingress merge* on replicated **root** cells only — not a parallel
reactivity path. Derived cells are never CRDT (deterministic functions of roots);
effects stay local (side effects do not replicate).

The dividing line is **concurrent-writer count, not local-vs-remote**: a
single-writer cell uses direct/delta whether it lives in one process or is mirrored
one-way to another; only a multi-writer cell needs CRDT. A replicated cell with a
single live writer degenerates to a near-free CRDT (no concurrent ops), so the kind
is chosen **statically by what the cell represents** — no dynamic per-cell
mode-switching is required. The attach/detach authority SM merely governs how many
writers are *live* against a replicated cell, not whether it is CRDT-capable.

### Cell = merge unit (the per-cell granularity fix, `#qcellmerge1`)

Making lazily cells the CRDT merge units delivers the standing per-component merge
intent (`project_agent_doc_percell_crdt_merge_intent`): replace the whole-document
`crdt::merge` (which splices content across cells — the `● Three agent-doc panes…`
block leaking into `agent:queue`) with independent per-cell convergence. Cross-cell
contamination becomes impossible by construction; the lazily cell graph supplies the
natural merge boundaries.

### Payoff alignment

- The backbone (`state_backbone.rs`) is already an event-sourced `EventLedger`
  projected through `lazily::ThreadSafeStateMachine`. Giving its events commutative
  CRDT op semantics makes the projection converge across replicas — the backbone
  *becomes* the CRDT layer, lazily the reactive projection on top. An upgrade of
  event semantics, not a new subsystem.
- It upgrades `#lazilystatesync` from a one-way Rust→Kotlin mirror into a **safe
  bidirectional replica** — exactly what editor-as-replica needs. When the editor
  writes, Rust and Kotlin become concurrent writers to the same cells, and
  CRDT-backed cells are what make that converge. So "lazily via CRDT" is the
  enabling mechanism for the editor-as-replica model above: lazily owns
  derivation/UI, CRDT owns convergence.

## Disk projection — kept, but demoted (Q1)

Disk `.agent-doc/crdt/<hash>.yrs` stays — as a **write-through durable projection
for crash/restart recovery**, not a coordination medium and not a source of
truth. If the supervisor dies mid-turn, its in-memory replica is recovered from
the last disk snapshot + git checkpoint (losing at most one flush, so keep
flushing at roughly today's cadence). Each editor's FFI node should also persist
its own replica locally so a plugin/IDE crash mid-lag does not lose un-synced ops.
The answer to "is the disk projection necessary?" is therefore: **yes for
durability, no as authority.**

## Phased path (focused cycles — supervisor/FFI-critical, not inline)

0. **Done (this session):** `#xdocsuper0` claim guard, `#queuestatemachine2`
   residue fix, `#xdocsuper1/3` hosting-epoch substrate — all on `main`.
1. **Authority state machine (`#mergestatemachine` extension).** Formalize
   Detached(git-authoritative, ephemeral CRDT) ↔ EditorAttached(multi-replica)
   keyed on the plugin attach/detach facts, riding the hosting-epoch backbone
   (`66ac6c64`). Headless path uses the ephemeral/git-authoritative CRDT.
2. **State-vector sync protocol.** Replace whole-`.yrs`-snapshot
   `merge_by_component` with incremental state-vector / update exchange in the
   FFI node ↔ supervisor channel. Per-replica `StateVector`, encode/apply updates,
   causal buffering (yrs-native).
3. **Editor-as-replica binding (FFI-first).** The cdylib hosts the per-editor yrs
   replica; thin IntelliJ/VS Code bindings forward `Document` deltas and apply
   remote updates (cursor/undo preserving). Plugin moves fully read/write-through
   the FFI node (completes `#dsqa` plugin-read-only direction).
3b. **lazily ⨉ CRDT cell kinds (`#lazilycrdt`).** Introduce the replicated
   (CRDT-backed) cell kind alongside local/direct cells; back the shared document /
   queue / hosting root cells with per-cell CRDT (= `#qcellmerge1` per-component
   merge), with lazily deriving on top. Upgrade `#lazilystatesync` to a
   bidirectional replica.
4. **State-vector commit barrier.** Replace the patch-ack in `finalize` with a
   "flush all live editors to SV=N, ack, snapshot, commit git" barrier. This is
   the durable fix for the `no_ack` / `ipc_proof_insufficient` /
   `retry_without_disk_write` class.
5. **Multiple-editor relay + awareness.** Supervisor as broadcast hub for N
   editor replicas; unique client-ids; optional ephemeral awareness channel.
   Checkpoint = consistent cut of live replicas; offline editors converge on
   reconnect.
6. **Disk demotion.** `.yrs` becomes write-through recovery projection only; each
   FFI node persists its replica locally for crash safety.

## Why a fresh focused cycle (not inline)

This is the supervisor-in-process + FFI critical path (real-time bidirectional op
sync, restart-recovery correctness, plugin thinning, multi-editor relay). It must
NOT be proven out in a contamination-victim session. Run each phase from a fresh
clean supervisor session, with SimWorld coverage for convergence-under-lag,
commit-barrier completeness, and multi-replica fan-out, plus a live multi-editor
repro as the only operator-gated step.

## Connections / supersedes

- Extends `#mergestatemachine`, `#xdocsuper1/3`, `#queuestatemachine`, `#dsqa`,
  `#qcellmerge1`, `#lazilystatesync`, `#lazilycrdt`.
- Supersedes the thread's "Option A vs B" framing (A and B are the Detached and
  EditorAttached states of one model).
- Root-fix target for the `ipc_proof_insufficient no_ack` / `File Cache Conflict`
  / stale-CRDT-residue classes recorded throughout `agent-doc-bugs2.md`.
