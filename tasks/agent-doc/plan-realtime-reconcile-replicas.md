# Plan: realtime-model replica reconciliation (Phases 2–3)

`#adoc-live-prompt-drift-operator-edit` · owner: agent-doc core

## Why

When the operator edits the assistant response in the live editor **before** the
cycle commits, the closeout used to wedge with `editor IPC did not prove the
write`. Root cause: several proof surfaces require the agent's **exact response
bytes** as convergence proof and misread an operator body-edit (or a live buffer
that moved past the ack capture) as "response missing" / `stale_source_buffer`,
forcing a needless editor redelivery that cannot prove against a lagging disk.

The deeper architecture goal (operator direction): **every writable replica
reconciles with the sender before accepting a change, and the turn blocks on a
bounded CRDT reconciliation in the realtime model until the sources are in
sync — never a wedge, never a lost operator edit.**

## Phase 1 — DONE (commit `8af0d850`)

Predicate fix in the realtime document model
(`agent-doc-document-realtime/src/write_policy.rs`):

- `response_converged_in_visible_target(base, candidate, target)` — a target that
  presents this cycle's `### Re:` heading(s) with a body is *converged* (operator
  body-edits win); no redelivery required. Wired at `ipc.rs:943` (semantic-merge)
  and `ipc.rs:987` (`#fintol2`).
- `buffer_presents_reference_response(reference, buffer)` — the operator's newer
  live buffer still carries the response. Drives
  `reconcile_ack_snapshot_to_newer_operator_buffer` (`ipc.rs`), which adopts the
  newer operator-authoritative buffer **forward** before the disk-write proof
  (`transport.rs`, ack path) so disk/snapshot/CRDT persist the operator's latest
  edits consistently — fail-closed only when the newer buffer genuinely dropped
  the response.

This removes the observed wedge but is still *point-in-time* proof, not a
reconcile loop, and still reasons over flat text + the whole-document yrs
`CrdtDoc`. Phases 2–3 replace that with the per-cell model and a real loop.

## Current architecture (as-found)

- **Text CRDT:** `yrs` 0.27 (`agent-doc-merge/src/crdt.rs`, `CrdtDoc`) — a
  whole-document `Y.Text`. Flat character sequence; structure is only marker
  comments, so concurrent edits to disjoint nodes can splice across
  component/item boundaries (the queue-text-in-exchange corruption class).
- **Per-cell CRDT:** `lazily` 0.13.1 in `agent-doc-merge/src/document_cell.rs` —
  `CellTree`/`CellMap`/`SemTree` structure + `TextCrdt` op-level merge +
  `reconcile` keyed LIS diff. **Already the production merge default**
  (`cell_merge_enabled()` ON) for `crdt::merge_by_component`. NOT yet used by the
  proof/convergence path.
- **Proof/convergence path:** `agent-doc-orchestration/src/write/{ipc.rs,
  ipc/transport.rs, converge.rs}` + realtime `write_policy.rs`. Uses text
  heuristics (`ack_content_contains_latest_response`, `response_materialized_in_content`)
  and byte-exact live-buffer matching — the source of the wedge class.
- **Two-stage recovery smell:** write-stage bails (`run_entry.rs:846`) while the
  actual convergence often runs later at commit stage (`git.rs:546`,
  `try_auto_recover_live_prompt_drift`). This stage split is a bug generator
  (auto-recovered log + wedge error in the same run).

## Phase 2 — replica proof/convergence on the per-cell model + bounded reconcile loop

Goal: replace the flat-text, point-in-time, two-stage proof with a **single
reconcile-before-accept step in the realtime model** built on the per-cell
`CellTree`/`TextCrdt` model.

Work items:

1. **Convergence predicate = per-cell state, not text bytes.** Add a realtime-model
   function that decides "in sync" from the per-cell model: project base /
   candidate / operator-buffer into `document_cell::project_document`, and treat the
   turn as converged when the exchange node(s) reconcile with no pending op
   (operator wins same-node; disjoint nodes both land). Replace
   `response_converged_in_visible_target`'s heading heuristic (Phase 1) with this
   as the authority; keep the heading check as a cheap fast-path.
   The realtime document graph should be a direct typed lazily
   `ThreadSafeContext<RealtimeDocumentSchema>` whose current-document, replica,
   receipt, and convergence slots/cells cannot be mixed with the short-lived
   cycle graph. Keep `CycleContext` for one-cycle IO/cache projections; use the
   typed realtime schema directly for cross-thread editor/watcher/actor
   authority instead of introducing a wrapper context type.
2. **One reconcile step, one stage.** Collapse the write-stage proof and the
   commit-stage `try_auto_recover_live_prompt_drift` into a single
   `reconcile_turn_against_live_buffer(file, base, candidate) -> Reconciled`
   entry in the realtime model, called once before the turn returns. It:
   - reads the newest operator-authoritative live buffer,
   - per-cell 3-way merges (base, candidate, operator-buffer) via
     `document_cell::merge_3way` (already the default),
   - **loops** re-reading the live buffer until the merge is a fixpoint (the
     operator stopped editing) or a bounded timeout,
   - returns the converged document to persist to disk + snapshot + CRDT.
3. **Bounded timeout → fail closed.** Reuse the existing typing-settle primitive
   (`agent_doc_debounce::await_idle_via_file`, 500 ms settle / 2 s timeout). On
   timeout without a fixpoint: retain the response pending, write nothing
   divergent, return retry (today's safety guarantee, reached via timeout instead
   of instant wedge). Keep the `#turnsaferecycle` stale-supervisor recycle hook.
4. **Delete the redeliver-then-prove-against-disk path** (`repair_ipc_decision_visible_state`
   disk-keyed redelivery) once the loop above owns convergence; the editor bridge
   (Phase 3) handles editor-side application.

Acceptance: an operator editing the response body during the write, and an
operator who keeps typing past the ack capture, both commit their edited response
with no wedge and no lost keystrokes; a genuinely dropped response still fails
closed; `make check` + a SimWorld reconcile-loop test green.

## Phase 3 — cell model across CP + editor shadow replicas via FFI

Goal: every replica (turn, CP/supervisor, editor plugin shadow) holds an
in-memory per-cell model and reconciles-before-accept.

Work items:

1. **Shared model behind FFI.** Expose the `CellTree` model + reconcile entry
   through `ffi.rs` (Shared Foundation) so JetBrains (JNA) and VS Code (FFI) drive
   the same Rust model — plugins stay thin event reporters. The editor's real
   buffer remains a linear OT authority (Document API); the plugin shadow model
   reconciles with the editor buffer before applying, via the existing
   shadow/ack protocol (ack-content sidecar, `live-buffer/` snapshots).
2. **CP in-memory model.** The route-owned supervisor holds the authoritative
   cell model and drives reconcile rounds when it sends a diff to the editor; if
   the editor's returned version is out of sync, run more CRDT rounds until in
   sync (bounded).
3. **VERIFY before retiring yrs — lazily `TextCrdt` sync semantics. → RESOLVED
   (2026-07-02): lazily HAS coordinator-free delta/frontier sync.**
   `op_level_merge` (`document_cell.rs`) only uses the 3-way-with-common-base
   entry (`TextCrdt::from_str` + `fork`), but lazily 0.13.1 also ships the full
   anti-entropy layer that true replica sync needs:
   - `lazily::TextCrdt::merge(other)` is a **commutative/associative/idempotent**
     state merge (tombstones order-independent) — proven for agent-doc's content
     shape by `agent-doc-document-realtime/src/replica_sync.rs` (4 convergence
     tests: disjoint inserts, delete+insert, concurrent same-char deletes, no-op).
   - `lazily::crdt_plane::CrdtPlaneRuntime` provides an HLC clock + per-peer
     `StampFrontier`, `sync_frame_since(frontier)` and `sync_reply(request)` — ship
     exactly the ops a peer's frontier is missing. That is **delta sync keyed on
     causal metadata**, the coordinator-free pull protocol (no shared base
     required).

   So yrs is **retirable for the replica model**: lazily's per-cell plane is a
   better fit than the whole-document `Y.Text` (per-cell isolation kills cross-cell
   splicing; HLC frontiers give causal-stability GC). Adoption path: hold one
   `CrdtPlaneRuntime` per replica, per-cell `TextCrdt`s behind the shared FFI,
   exchange `CrdtSync` frames. **Still do not retire yrs in the same change** as
   the first plane adoption — migrate persistence (`.yrs` → lazily op-log
   snapshot) once the plane model is the authority and its serialization is
   proven, to keep the cutover reversible.
4. **Structured persistence.** Once the cell model is the authority, persist the
   per-cell structure (not a flat text blob) so restart/merge is per-node.

Acceptance: operator + agent + CP concurrent edits across editor and pane
converge with no cross-cell splicing and no wedge; editor shadow reconciles
before applying; documented decision on lazily-vs-yrs for persistence/transport.

## Non-goals / guardrails

- No release ceremony (crates.io/PyPI/tag) until per-cell CRDT cutover is proven —
  local `cargo install` + `lib-install` only (see repo release memory).
- Never overwrite operator-visible text with `content_ours`/snapshot/ACK content
  when it would drop operator edits — fail closed or retry through the editor.
- Keep the reconcile logic in the realtime document model
  (`agent-doc-document-realtime`), not the orchestration or skill layer.

## Open questions

- ~~lazily `TextCrdt` delta/state-vector API (Phase 3 item 3) — the gating
  check.~~ **RESOLVED:** lazily ships `CrdtPlaneRuntime` (HLC + `StampFrontier` +
  `sync_frame_since`/`sync_reply`) and commutative `TextCrdt::merge`; convergence
  proven by `replica_sync.rs`. Phase 3 is unblocked.
- Whether `document_cell::project_document` node keys are stable enough to be the
  convergence identity across a live operator edit that renames a heading.
- Editor-side shadow reconcile: JetBrains Document API listener granularity vs
  the ack-sidecar round-trip latency budget.
