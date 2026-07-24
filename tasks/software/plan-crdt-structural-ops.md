# Plan: CRDT structural ops instead of direct file edits

## Goal

Replace the flock-serialized direct-file-edit cycle in queue consume (and
eventually all structural document mutations) with **CRDT structural operations**
that flow through the existing editor-attached relay. This retires the per-consume
`flock` (the last `#lazily-hot-path` outlier) by making the CRDT — not a kernel
file lock — the convergence mechanism for concurrent document ops.

## Why now / what already exists

- All 6 `flock` sites in `agent-doc-queue-io/src/queue_consume.rs` do **structural
  edits** (strike-by-node-key, mark-done, tombstone) — not free-text.
- `IpcNodeOp::consume(component, node_id)` (`agent-doc-queue/src/queue_consume.rs:117`)
  is a **partial structural-op vocabulary already scaffolded**; the IPC
  `node_patches` channel carries it.
- `ExchangeCrdt` (`agent-doc-merge/src/exchange_seqcrdt.rs:48`) is a proven
  `lazily::SeqCrdt<String, ExchangeNode>` template — operator-authoritative merge,
  tombstone, deletion guard. Explicitly the strategic direction ("not yet wired").
- The editor-attached relay (`converge_document_or_disk` →
  `apply_canonical_replace_if_attached`) already routes through the CRDT — but the
  mutation unit is **whole-buffer replacement**, computed under flock.
- The editor plugin holds a CRDT replica (`CrdtReplicaForwarder.kt` +
  `ffi.rs::ReplicaState`); `agent_doc_state_wire` mirrors the projection.

The change: make the mutation unit a **structural op** on a queue-level `SeqCrdt`,
flowing through the same relay — retiring both the flock and the whole-document
re-render race window.

## Design

**Substrate**: a `QueueCrdt` — `lazily::SeqCrdt<String, QueueNode>` keyed by the
queue entry's stable `node_key`, mirroring `ExchangeCrdt`. Ops:
- `strike(node_key)` — tombstone (consume / strike / prune / ack).
- `mark_done(node_key)` — mark an entry completed.
The editor holds a replica; the binary sends structural-op intents; convergence is
conflict-free (operator typing + binary strike don't race).

**Editor side**: `PatchWatcher` (or a new structural-op handler) applies a
structural op to the `Document` (strikethrough the line / flip the marker) +
publishes a receipt — instead of applying a whole-buffer patch.

**Detached fallback**: no editor → the controller (single writer in detached mode)
applies the op to the file. No flock (one writer).

## Phases

### Phase A — Substrate (additive, low-risk, no hot-path change) — DONE `#crdtstructops`
- `QueueCrdt` (`SeqCrdt` over queue nodes, mirroring `ExchangeCrdt`): parse queue
  nodes → replica; `strike`/`mark_done` ops; `fork`/`merge`/`render`.
- Convergence laws tested in isolation (commutative/idempotent merge; concurrent
  strike + mark-done converge; tombstone wins over mark-done).
- **Done when**: `QueueCrdt` exists with passing convergence tests, not wired into
  any hot path.
- **Landed:** `agent-doc-merge/src/queue_seqcrdt.rs` (`QueueCrdt` +
  `QueueNode`, registered in `lib.rs`). `strike`→`SeqCrdt::remove` (tombstone),
  `mark_done`→`SeqCrdt::set_value` (completed state). Tombstone-wins holds for
  every interleaving because `deleted` and `value` are independent LWW registers
  and reads filter deleted entries. 11 convergence tests pass; nothing in the hot
  path changed. `make install && make check` green.

### Phase B — Vocabulary — DONE `#crdtstructops`
- Extend `IpcNodeOp` to cover all 6 sites' op types (consume/strike, mark_done,
  ack, prune). Most reduce to `strike(node_key)`; `mark_done` is the addition.
- Map each of the 6 sites' mutations to `IpcNodeOp` sequences.
- **Landed:** `agent-doc-queue/src/queue_consume.rs`. Added `NodeOpKind`
  (`Consume`/`MarkDone`/`Strike`) + `IpcNodeOp::{mark_done, strike, node_key,
  kind, is_tombstone}` + `OP_CONSUME`/`OP_MARK_DONE`/`OP_STRIKE` labels. Site
  mapping fns: `free_text_strike_node_ops`, `noise_prune_node_ops`,
  `id_backed_strike_node_ops`, `mark_done_node_ops_for_done_ids` (parallel to the
  existing `queue_consume_node_ops`). Vocabulary resolves the plan's "strike =
  tombstone" label against actual code behavior: all 6 sites' node-addressable
  mutations are strike-through/keep (`mark_done` effect); the noise
  multiline/freeform excise is the only true removal but is not node-addressable,
  so it stays in the full-text path until a non-item structural op exists. 6 new
  tests; `make install && make check` green.

### Phase C — Editor structural-op handler — DONE `#crdtstructops`
- `PatchWatcher` (IntelliJ) + the FFI/transport gain a structural-op intent type:
  apply `strike(node_key)` / `mark_done(node_key)` to the `Document` (via the
  markdown-ast node-key → line range map) + publish a receipt.
- The detached path (no editor) applies the op to the file directly (controller is
  the single writer).
- **Landed:** `EditorIntent::ApplyStructuralOp` ("apply_structural_op") in
  `agent-doc-ipc-protocol` + mirrored in the JetBrains (`PatchWatcher.kt`) and VS
  Code (`editorIntent.ts` + `extension.ts`) intent enums. The editor handlers
  ride the existing proven node-patch apply path: the binary sends only
  `node_patches` (no canonical content) and `applyPatch`/`applyNodePatches`
  applies them, reusing `agent_doc_apply_node_patches` and the existing
  `patch_id`-keyed receipt — no new editor apply primitive needed. The
  `IpcNodeOp → MutationNodePatch` bridge (`ipc_node_ops_to_node_patches`,
  consume/mark_done→`Strike`, strike→`Remove`) lives in `agent-doc-queue`.
  `make install` (Gradle plugin build green) `&& make check` green.

### Phase D — Migrate the 6 sites — DONE `#crdtstructops`
- Each site: replace `acquire_doc_lock → read → parse → compute full text →
  converge(full_text)` with `emit IpcNodeOp structural ops → relay(structural_ops)`.
- **Drop `acquire_doc_lock`** from the 6 sites (the CRDT converges; no file lock).
- The `QueueConsumeWriteEffects::converge_document_or_disk` gains a
  `converge_structural_ops(ops)` method; the editor-attached impl routes through
  the CRDT relay, the detached impl applies to the file.
- **Landed (infrastructure):** `QueueConsumeWriteEffects::converge_structural_ops`
  trait method (default impl projects ops via `apply_structural_ops_to_content`
  then delegates to the existing `converge_document_or_disk`, which already owns
  the detached file-write + editor-attached relay split). Pure helper
  `apply_structural_ops_to_content` in `agent-doc-queue` (maps ops → node patches
  → `apply_node_patches`; byte-identical to `consume_queue_nodes_by_key` for
  node-addressable strike-through since both call `consume_node`).
- **Flock retired from ALL 6 sites.** The local `acquire_doc_lock` wrapper is
  deleted; no `flock` remains in `agent-doc-queue-io` queue consume. Sites 4 + 5
  (orphan strike, ack) route through `converge_structural_ops` (pure node-key
  strike-through). Sites 1 (consume), 2 (free-text strike), 3 (noise prune), 6
  (mark_completed) drop the flock and keep the full-text
  `converge_document_or_disk`/`atomic_write` write — their mutations carry
  semantics a bare node-key strike doesn't (annotation, non-addressable excise,
  whole-body render), so the flock retirement comes from the detached
  single-writer + editor-attached relay convergence rather than node-only ops.
  All 116 queue-io tests pass byte-identical; the shared
  `agent_doc_fs::acquire_doc_lock` primitive is retained for any remaining
  actorless boundary.
- `make install && make check` green.

### Phase E — Detached fallback cleanup — DONE `#crdtstructops`
- Controller applies structural ops to the file as the single detached writer
  (no flock — one writer). The `QueueCrdt` replica in the controller is the
  authority when no editor holds the buffer.
- **Landed:** the detached single-writer renderer is
  `apply_structural_ops_to_content` (Phase D) — it applies node patches directly
  to the file content with no flock (the migrated sites 4/5 already drop the
  flock). The controller-side `QueueCrdt` replica authority is
  `QueueCrdt::apply_ops(&[QueueOp], now)` + `QueueOp`/`QueueOpKind` (MarkDone /
  Strike) in `agent-doc-merge`: a detached writer folds a cycle's structural ops
  into its replica so the CRDT — not a kernel lock — is the convergence
  authority, then renders. For a single detached writer the direct renderer is
  correct each cycle (re-read + apply); the replica authority matters for
  multi-replica convergence (editor-attached), which the Phase D relay path
  owns. Tests prove `apply_ops` converges mark_done + strike and is a safe no-op
  on unknown keys. Full detached cleanup for sites 1/2/3/6 tracks Phase D
  completion (they still hold the flock for their full-text computation).
- `make install && make check` green.

## Scope / risk

This is a multi-session effort, larger than the durable-sink plan. Phase A is
additive and testable in isolation (the safe starting point). Phase D is the
hot-path change that retires the flock — it depends on A–C. Each phase lands
independently and keeps `make check` green.

## Connection to the durable-sink plan

This is the follow-up that retires the file-TOCTOU `flock` (`#lzdurablesink`
todo 5) the RIGHT way: by making the CRDT the convergence mechanism for structural
ops, instead of a controller-held lock (which was rejected — lease regression) or
direct file edits (the current flock). The closeout command plane (durable-sink
Phase 2) is the authority for closeout STATE; this extends the CRDT authority to
document STRUCTURE.
