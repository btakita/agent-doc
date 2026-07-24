# Session prompt: CRDT structural ops — execute the 5 phases

## Context

The agent-doc durable-sink plan (`tasks/software/plan-lazily-durable-effect-sinks.md`) is complete: closeout authority (all 4 transitions + reads + owner claim/release) routes through the lazily command plane, `make check` green. The one remaining outlier is the per-consume **file-TOCTOU `flock`** (6 sites in `agent-doc-queue-io/src/queue_consume.rs`), kept by design (correct kernel primitive for direct file edits) until the system uses CRDT structural ops instead.

**This session executes `tasks/software/plan-crdt-structural-ops.md`** — replacing direct file edits with CRDT structural operations that flow through the existing editor-attached relay, retiring the `flock`.

**Key dogfooding rule (learned the hard way):** the controller runs the *installed* `agent-doc` binary. Run **`make install` before `make check`** whenever controller/command-plane/CRDT code changes, or integration tests that spawn the controller will use a stale binary.

## What's already scaffolded (don't rebuild)

- **`ExchangeCrdt`** (`agent-doc-merge/src/exchange_seqcrdt.rs:48`) — the `SeqCrdt` template to mirror: `from_nodes`/`from_inner`/`nodes`/`ids`/`contains`/`push_back`/`remove`(tombstone)/`fork`/`merge`/`render`, + `merge_exchange_crdt` (operator-authoritative merge with deletion guard). It wraps `lazily::SeqCrdt<String, ExchangeNode>` keyed by `ExchangeNode::node_id()`.
- **`IpcNodeOp`** (`agent-doc-queue/src/queue_consume.rs:117`) — partial structural-op vocab: `{component, node_id, op}`, with `consume(component, node_id)` + `queue_consume_node_ops(node_keys)`. The IPC `node_patches` channel carries it.
- **The editor-attached CRDT relay**: `converge_document_or_disk` (`agent-doc-write-converge-io/src/lib.rs:4079`) → `apply_canonical_replace_if_attached` (`agent-doc-document-realtime-io/src/lib.rs:1962`) — already routes attached docs through the CRDT, but the mutation unit is **whole-buffer replacement**. The editor applies via `PatchWatcher.applyPatch` (`editors/jetbrains/.../PatchWatcher.kt:798`) and publishes a receipt through `StateProjectionBridge`.
- **`QueueConsumeWriteEffects`** trait (`agent-doc-queue-io/src/queue_consume.rs:29`): `current_document_content`/`force_disk_document_content`/`atomic_write`/`converge_document_or_disk`. Production impl: `RuntimeQueueConsumeWritebackEffects` (`agent-doc-document-realtime-io/src/lib.rs:654`).
- **The 6 `flock` sites** (all structural edits — strike/mark-done, NOT free-text):
  1. `consume_queue_prompts_with_options` (line 200) — strike leading free-text/done-id heads, mark completed, re-render.
  2. `strike_answered_free_text_queue_heads` (364) — strike answered free-text heads (#ftstrike).
  3. `prune_noise_queue_heads` (564) — strike non-drainable noise heads.
  4. `strike_orphan_id_backed_queue_head` (636) — strike one orphan `do [#id]` head.
  5. `acknowledge_open_id_backed_queue_head` (722) — strike (ack) an id-backed correction head.
  6. `mark_completed_queue_prompts_for_done_ids` (803) — mark entries completed for done-ids.

## The 5 phases (each lands independently, `make check` green)

### Phase A — QueueCrdt substrate (additive, no hot-path change)
- Build `QueueCrdt` mirroring `ExchangeCrdt`: `lazily::SeqCrdt<String, QueueNode>` keyed by the queue entry's stable `node_key`, with `strike(node_key)` (tombstone) + `mark_done(node_key)` ops, `fork`/`merge`/`render`.
- Put it in `agent-doc-merge` (alongside `exchange_seqcrdt.rs`) or `agent-doc-queue` (where `IpcNodeOp` lives).
- Convergence-law tests (in isolation, no editor): commutative/idempotent merge; concurrent strike + mark_done converge; tombstone wins over mark_done; fork→diverge→merge converges.
- **Done when**: `QueueCrdt` + tests pass; nothing in the hot path changed.

### Phase B — Extend the op vocabulary
- Extend `IpcNodeOp` to cover all 6 sites' ops. Most reduce to `strike(node_key)` (= tombstone). Add `mark_done` (the one new op). Map each site's mutation to an `IpcNodeOp` sequence.

### Phase C — Editor structural-op handler
- Add a structural-op intent type to the IPC payload (alongside `ApplyCanonical`): `ApplyStructuralOp(IpcNodeOp)`.
- `PatchWatcher` (IntelliJ) + FFI/transport: apply `strike(node_key)` / `mark_done(node_key)` to the `Document` via the markdown-ast node-key → line-range map, + publish a receipt (same `StateProjectionBridge` path).
- VS Code plugin equivalent if applicable.

### Phase D — Migrate the 6 sites (drop the flock)
- Each site: replace `acquire_doc_lock → read → parse → compute full text → converge(full_text)` with `emit IpcNodeOp structural ops → relay(structural_ops)`.
- Add `QueueConsumeWriteEffects::converge_structural_ops(ops)`; editor-attached impl routes through the CRDT relay, detached impl applies to the file.
- **Remove `acquire_doc_lock`** from the 6 sites. Keep the shared `agent_doc_fs::acquire_doc_lock` primitive for any remaining actorless boundary.

### Phase E — Detached fallback cleanup
- When no editor holds the buffer, the controller (single detached writer) applies structural ops to the file directly — no flock (one writer). The `QueueCrdt` replica in the controller is the authority.

## Constraints

- Each phase: `make install && make check` green before committing (clippy `--all-targets` + nextest + sim-medium + version-sync + lean + tla).
- Commit + push each phase to `main` (tag `#crdtstructops`).
- Preserve the editor plugin's CRDT buffer agency — the editor is a CRDT *peer*, not a passive projection. The binary's structural ops CONVERGE through the editor's CRDT, they don't clobber it.
- Keep the actorless/detached fallback working (tests that run without an editor must pass).
- Update `tasks/software/plan-crdt-structural-ops.md` progress after each phase.

## Start here
Phase A: read `agent-doc-merge/src/exchange_seqcrdt.rs` (the template), then `agent-doc-queue/src/queue_consume.rs:117` (`IpcNodeOp` + `queue_consume_node_ops`), then build `QueueCrdt` + convergence tests.

<!-- agent:exchange -->
### Re: Phase A — QueueCrdt substrate DONE — glm-5-2

Phase A landed. `QueueCrdt` lives in `agent-doc-merge/src/queue_seqcrdt.rs` (registered in `lib.rs`), mirroring `ExchangeCrdt`: a `lazily::SeqCrdt<String, QueueNode>` keyed by the durable queue `node_key` from `agent_doc_markdown_ast::mutations::item_nodes`.

**Ops:**
- `strike(node_key)` → `SeqCrdt::remove` (tombstone = consume/strike/prune/ack).
- `mark_done(node_key)` → `SeqCrdt::set_value` (completed `- ~~text~~` state, read-modify-write preserving text).
- Plus `from_nodes`/`from_inner`/`nodes`/`ids`/`contains`/`get`/`push_back`/`fork`/`merge`/`render`.

**Why tombstone-wins holds for free:** `SeqCrdt` keeps `deleted` and `value` as independent LWW registers, and `order`/`values`/`contains`/`get` all filter deleted entries. So a strike makes the node observationally vanish no matter what a concurrent (or even later) `mark_done` writes to its value — the convergence law the plan specifies, with no bespoke deletion guard.

**Verification:** 11 convergence-law tests pass (commutative/idempotent merge; concurrent strike + mark_done converge both merge orders; tombstone wins even when mark_done happens later; fork→diverge→merge; idempotent local apply; duplicate-key collapse; byte-stable item-only round-trip; completed source loads as `done` not tombstone). `make install && make check` green (clippy + nextest + sim-medium + version-sync + lean + tla). Nothing in the hot path changed — the 6 `flock` sites are untouched.

**Committed + pushed to `main`** (`#crdtstructops`): submodule `agent-doc` `94c2676d3` + superproject `aa422afee`; plan file marks Phase A DONE.

Next: Phase B — extend `IpcNodeOp` to cover all 6 sites' op types (most reduce to `strike(node_key)`; add `mark_done`).
<!-- agent:boundary:958e202d -->
<!-- /agent:exchange -->
