# Plan: Cross-document supervisor contamination (`#xdocsuper`)

## Operator ask

> Please fix the cross document supervisor contamination. This was from the
> session that started off running corky.md, but somehow run agent-doc-bugs2.md.

Live repro this session (2026-06-25): one Claude Code pane (`%73`, session
`b26b9957`) ran `/agent-doc` on `equityfundingsource.md` → `corky.md` →
`agent-doc-bugs2.md` in sequence (SessionStart index `#S9003`/`#S9009`). The
route-owned supervisor started for the first document persisted as the pane's
parent and was reused for each later document.

## Root cause

- The **authoritative actor record** is keyed per *file*
  (`route/authoritative_actor.rs::load_authoritative_actor_binding(file, …)`) and
  has a robust **harness**-mismatch guard
  (`route_authoritative_actor_harness_mismatch_{stale,deferred}`) — but **no
  document-identity guard**.
- Under the 08b supervisor-in-process model, `start --route-owned` is **one
  supervisor process per pane**, hosting cycles in-process. When the same pane
  switches documents, the single supervisor keeps serving the new document while
  still holding the **prior document's per-document in-memory state** (CRDT
  overlay, queue model, drain lease, cycle journal).
- That stale in-memory state replayed against the new document is precisely the
  recurring churn class: `live_prompt_drift_after_preflight`, post-commit
  `postcommit_worktree_check match=false`, and answered-head residue re-injection
  (`feedback_parent_supervisor_stale_crdt_reset_nopreserve_then_restart`). The
  per-`reset --from-current` and `git checkout HEAD` recoveries we keep applying
  are treating the symptom; this is the structural source.

## The claim-guard gap (operator: "how was the supervisor even allowed to claim a document another supervisor claimed?")

The claim binding invariant is **pane/session-keyed, not document-supervisor-keyed**:

- `claim.rs::cross_session_decision(pane_session, configured, configured_alive, force)`
  returns `Accept` (pane session matches), **`AcceptStale` (configured/prior
  session not alive → auto-force)**, `AcceptForce` (`--force`), else `Reject`.
- Its invariant is "never commandeer another **document's** pane" (SPEC §8.5) —
  it has **no** "is another **live supervisor** already hosting this document?"
  check. When the harness relaunches the agent into a fresh pane and the prior
  owner pane/session is classified stale (the `● Three agent-doc panes exist …
  %73 is the stale prior owner` contamination block in this doc is the live
  trace), `AcceptStale` auto-grants the reclaim.
- A document-level `supervisor_lease` **does** exist
  (`project_controller.rs::load_supervisor_lease_from_db` /
  `supervisor_lease_is_fresh_or_alive`, reconciled in
  `reconcile_supervisor_leases_after_restart`), but **`claim.rs` never consults
  it**. So two supervisors (old + relaunched) can both believe they own the
  document across the relaunch window — the contamination.

**Re the operator's FFI↔JB IPC-state-machine hypothesis:** partially right. The
merge-control IPC SM (`merge_control_state_machine.rs`) governs *write-ownership*,
not *claim*, so it is not the direct trigger. But the same-era supervisor-in-
process (08b) migration is what lets a single supervisor process host multiple
documents in-process; combined with claim being pane/session-keyed (never
checking the supervisor lease), that is the contamination surface. The instinct
points at the right neighborhood; the exact hole is the claim guard.

## Fix approach (focused cycle)

0. **`#xdocsuper0` — claim consults the document supervisor lease.** Before
   `AcceptStale` (and on the route reclaim path), check
   `supervisor_lease_is_fresh_or_alive` for the document: if a FRESH lease is
   held by a live supervisor, `Reject` (or require `--force`) even when the prior
   pane/session looks stale. This closes the "two supervisors own one document"
   window directly.


1. **`#xdocsuper1` — document-switch detection + state-reset guard.** When a
   route-owned supervisor is asked to host/dispatch a document whose path-hash
   differs from the one it last hosted on that pane, drop that supervisor's stale
   per-document in-memory state (CRDT overlay, queue projection, drain lease,
   cycle cache) and reload from disk before the first cycle — i.e. treat a
   document switch like a fresh `reset --from-current` for the incoming document.
   Mirror the existing harness-mismatch guard's structure in
   `route/authoritative_actor.rs` + the supervisor host loop
   (`project_controller/rpc.rs`, `supervisor/`).
2. **`#xdocsuper2` — SimWorld coverage.** Deterministic scenario: a route-owned
   supervisor hosts doc A, then is handed doc B; assert B's first cycle reads B's
   on-disk state (no A overlay), and A's state is untouched. Add a contamination
   regression asserting no `live_prompt_drift` from a sibling document's stale
   overlay.
3. **`#xdocsuper3` — model the binding as state (optional).** Fold the
   pane→document host binding into the state backbone (`state_backbone.rs`) so a
   document switch is an explicit transition, not implicit reuse — same direction
   as `#queuestatemachine` / `#mergestatemachine` / `#waitmachine`.

## Why a fresh focused cycle (not inline)

This is supervisor-in-process-critical (`project_controller`, `route`,
`supervisor`) and needs new SimWorld infra. It must NOT be implemented in a
session that is itself a contamination victim (this `agent-doc-bugs2.md` session
on a degraded route-owned supervisor) — run it from a fresh session/pane on a
clean supervisor. Same rationale as `#agentreloadrestart` Phase 1b
(`feedback_deep_supervisor_refactor_fresh_focused_cycle`).

## Design note: would a reactive state machine help? (`#xdocsuper3` re-scope)

Operator question (2026-06-25): *"Would a reactive state machine help to manage
the supervisor document bindings?"* Short answer: **yes for the document-switch
/ hosting problem; no (over-engineering) for the claim guard.** Critically, the
reactive substrate **already exists** — this is an extension, not greenfield.

### The reactive substrate is already built (`state_backbone.rs`)

`agent-doc-orchestration/src/state_backbone.rs` is already an **event-sourced,
per-`document_hash`, epoch'd, idempotent** reactive projection. Its `StateEvent`
variants span the full lifecycle (`PreflightStarted`, `ResponseCaptured`,
`WriteApplied`, `CommitObserved`, `SessionCheckPassed`, `OwnerGenerationChanged`,
`AgentRestartPerformed`, `RoutePaneObserved`, `RouteReadinessObserved`,
`DispatchProofObserved`, …), `document_epoch(document_hash)` advances per
document, and a re-emit is idempotent (does not bump the epoch). So "model the
binding as state" lands in a backbone that already exists — the pane→document
host binding is the missing event variant, not a new subsystem.

This is distinct from the **pure-transition** SMs
(`queue_item_state_machine`, `merge_control_state_machine`, `cycle_state_machine`),
which are pure `max`/join decision functions that make a bad *transition* a
no-op. The backbone is the reactive/event-sourced layer those SMs sit atop.

### Three sub-problems, two tools

- **`#xdocsuper0` — claim guard. NOT an SM.** The hole is a missing predicate:
  `claim.rs::cross_session_decision` never consults
  `supervisor_lease_is_fresh_or_alive` (the lease already lives in SQLite via
  `state_store.rs::load_supervisor_lease_from_db`). A pure guard on
  `AcceptStale` closes the two-supervisors-one-document window. Framing this as
  a reactive SM over-engineers a one-line check. **Do this first** — highest
  leverage, lowest risk, no new infra.

- **`#xdocsuper1` — document-switch reset. This is where an SM pays off.** The
  illegal state is "hosting DocB while still holding DocA's stale in-memory
  overlay (CRDT / queue projection / drain lease / cycle journal)." Modeling the
  hosting lifecycle explicitly (`Detached → Hosting(doc) → SwitchDetected →
  Resetting → Hosting(doc')`) makes the illegal state *unreachable by
  construction* — the same structural-impossibility guarantee
  `queue_item_state_machine` gives for duplicate queue items.

- **`#xdocsuper3` — fold the binding into the backbone. Re-scope from
  "optional" to "the implementation vehicle for `#xdocsuper1`."** Express the
  reset **as a backbone transition** — e.g. a `SupervisorHosting {
  document_hash, pane_session, lease_epoch }` / `DocumentSwitched` event feeding
  the projection — **not** as ad-hoc reset code in the supervisor host loop.
  Otherwise `#xdocsuper1` becomes another implicit-reset path that can be
  bypassed under IPC degradation: the same class of bug it is fixing. The
  backbone's epoch already advances on `OwnerGenerationChanged`; a document
  switch is the same kind of epoch advance that makes a stale-overlay replay a
  no-op. **Recommendation: implement `#xdocsuper1` *inside* `#xdocsuper3`.**

### Necessary but not sufficient (honest tradeoff)

An event-sourced SM only holds if emitting the switch event + reload is the
**first** step on every document handoff, enforced **structurally** — the
projection's `Hosting` state must be constructable only from a fresh switch
event paired with an on-disk reload, not an optional reconciliation pass. The
lesson from `#queuestatemachine`: convergence must **drive** to the lawful
state, not **patch** after the fact. A skipped emit under IPC drift reintroduces
the hole, so the SM is necessary but the structurally-enforced ordering is what
actually closes it.

### Recommendation

Keep `#xdocsuper0` as the standalone guard (ship first). Fold `#xdocsuper1` into
`#xdocsuper3` as a backbone transition. This matches the operator's "represent X
as a state machine so the bad state is impossible" theme across
`#queuestatemachine` / `#mergestatemachine` / `#waitmachine`, and reuses the
backbone that already models every other document-scoped lifecycle fact.

## Implementation findings (xdocsuper1/3)

Investigated the live code in the `xdocsuper-statereset` worktree before coding.

### Where the stale per-document in-memory state lives + is replayed

- The supervisor-in-process model holds per-document state in the **process-global,
  append-only `EventLedger`** (`agent-doc-orchestration/src/state_backbone.rs:258-349`),
  surfaced through the FFI singleton `STATE_LEDGER`
  (`src/ffi.rs:1701-1710`). Plugins/supervisor report facts via
  `agent_doc_record_state_event` (`src/ffi.rs:1771`) and read the
  per-document projection via `agent_doc_state_projection` (`src/ffi.rs:1726`) /
  `agent_doc_state_subscribe` (`src/ffi.rs:1846`).
- The per-document overlay is `DocumentStateProjection`
  (`state_backbone.rs:384`), rebuilt **on demand** from the ledger by
  `EventLedger::project_document(document_hash)`
  (`state_backbone.rs:307-324`). It is keyed by `document_hash` in
  `StateBackboneProjection.documents` (`state_backbone.rs:353`).
- The **answered-head residue** lives in `QueueProjection`
  (`state_backbone.rs:770-816`): `active_head`, `heads`, and especially
  `completed_heads` (`state_backbone.rs:776`, accumulated in `apply_completed`
  at `:804-815`). Replay point: every call to `project_document` re-applies
  **all** accepted queue facts for the document with **no generation/epoch gate**
  (contrast the transport/route/supervisor facts, which ARE generation-gated via
  `current_generation_matches` at `state_backbone.rs:691`). So a stale queue
  overlay (answered heads from a prior hosting of the same or a sibling document)
  is faithfully replayed into the fresh cycle's projection.
- **The structural gap:** there is no `pane_session → currently-hosted document`
  binding in the backbone and no event that marks a host/switch. So the projection
  cannot tell "this queue overlay belongs to a hosting that is now over." The
  authoritative-actor layer (`route/authoritative_actor.rs`) has a robust
  *harness*-mismatch guard (`route_authoritative_actor_harness_mismatch_{stale,
  deferred}`) but **no document-identity / hosting-epoch guard**, and it does not
  feed the backbone.

### Discrepancy vs the plan's hypothesis

The plan describes the stale state as "CRDT overlay, queue model, drain lease,
cycle journal" held as mutable supervisor fields keyed by path. In this codebase
that state is **already event-sourced** into the single `EventLedger`/projection
substrate rather than spread across ad-hoc mutable supervisor fields — so the fix
is not "drop a cached struct on switch" but "make the backbone projection
**epoch-gate the queue domain on the hosting binding**", exactly the
`#xdocsuper3`-inside-`#xdocsuper1` direction the design note recommends. The
queue domain was the one document-scoped domain with no generation/epoch gate,
which is why it (and only it) leaks across hostings.

### Fix shape (implemented)

1. New `StateFact::SupervisorHosting { document_hash, pane_session, lease_epoch }`
   (Supervisor domain). The projection tracks, per document, the
   `(pane_session, hosting_epoch)` currently hosting it.
2. A host event is a **switch/fresh-host** when the pane_session changes for the
   document, or the same pane re-hosts at a higher `lease_epoch`. On a
   switch/fresh-host the projection **resets the queue overlay to empty** (drops
   `active_head`/`heads`/`completed_heads`) — making a stale-overlay replay a
   no-op *by construction*, the same guarantee `queue_item_state_machine` gives
   for duplicate queue items.
3. Queue facts are now **hosting-epoch gated**: a queue fact produced under an
   older hosting epoch than the document's current hosting is rejected
   (recorded in `rejected_stale_events`, mirroring the generation gate). This is
   what makes the ordering structurally enforced — the `Hosting` queue state is
   only constructable from facts at the current hosting epoch, so a skipped reset
   cannot reintroduce the hole: the stale facts are rejected even if the reset
   event is replayed/reordered, because they carry the old epoch.
4. The reset is the FIRST thing applied on a host transition (handled inside the
   single `apply` reducer, before any later queue fact for the new hosting can be
   accepted), and the FFI `agent_doc_record_state_event` path that the live
   supervisor uses appends the `SupervisorHosting` fact through the same reducer.

## Relationship to other work

The structural sibling of `#queuestatemachine` (per-item dup-impossibility),
`#mergestatemachine` (write-ownership), and `#waitmachine` (bounded waits): the
operator's "represent X as a state machine so the bad state is impossible"
direction. Here X is the pane→document supervisor host binding.
