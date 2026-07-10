# Plan: Compact converge should CAS-retry on `retry_crdt_merge`

`#compactcrdtretry` · `#agent-doc-bug`

## Symptom

`Compact Exchange` (JB action → `agent-doc compact <FILE> --commit`) fails closed
with exit 1 while the operator is still editing the document:

```
[agent-doc] Tagged pre-compact state as agent-doc/<doc>/pre-compact-N
Error: project controller command `crdt_cpc_write` failed: CPC relay write refused
for <doc>: expected_hash=<A> current_hash=<B> recovery=retry_crdt_merge
```

The compaction is computed against document content at hash `A`; the operator keeps
typing (or a CRDT re-encode lands), the live document moves to hash `B`, and the
compare-and-swap CPC write is refused because `expected_hash(A) != current_hash(B)`.
The refusal names its own recovery — `retry_crdt_merge` — but compact does not retry.

## Root cause

`agent-doc-compact-io::apply_compacted_document` writes through
`runtime_effects().try_editor_converge(file, compacted, source_content, "compact")`.
`source_content` is the pre-write CAS baseline. When the live document has drifted,
`agent-doc-crdt-relay-io` refuses the write (`lib.rs:1144`, `recovery=retry_crdt_merge`)
and the error propagates all the way out of `run()` as exit 1.

The route-queue enqueue path already solved exactly this class:
`agent-doc-route-io/src/queue_dispatch.rs` wraps its write in a bounded CAS loop —
`is_retryable_crdt_merge_error()` + `ROUTE_QUEUE_MAX_WRITE_ATTEMPTS` (3) — that
re-resolves current content, re-prepares the update, and retries. Compact has no
equivalent.

## Why it is not a one-line port

Compact has a side effect the route-queue path does not: `save_archive()` writes an
archive file (and the compacted summary embeds that archive path) *before* the
converge. A naive retry that re-runs `run_component_compact_with_options` re-creates
the archive, piling up duplicate `.agent-doc/compact-archive/*` files per attempt.

## Fix

1. **Make archive creation retry-idempotent.** Either (a) create the archive once and
   reuse its path across retries, or (b) content-address the archive so an identical
   re-save is a no-op. Prefer (a): compute archive + summary once, then retry only the
   converge/snapshot step.
2. **Bounded CAS retry around the converge.** On `is_retryable_crdt_merge_error`,
   re-resolve current content via `try_resolve_current_document_content`. If the
   component being compacted (the `exchange` tail, or the named component) is unchanged
   between `source_content` and the fresh current, re-apply the already-built summary
   onto the fresh current (`comp.replace_content(fresh_current, visible_content)`),
   refresh `snapshot_content`/`authoritative` from that re-merge, and retry the converge
   with fresh current as the new CAS baseline. Cap attempts (reuse
   `ROUTE_QUEUE_MAX_WRITE_ATTEMPTS`-style constant).
3. **Fail closed cleanly when the target itself changed.** If the exchange (or named
   component) being compacted changed during compaction, do not silently recompact over
   the operator's new text — bail with an actionable message
   (`document changed during compaction (component=<x>); re-run compact`) instead of the
   raw CPC hash error.
4. Log each retry with an ops.log marker analogous to `route_dispatch_queue_retry`
   (`compact_converge_retry file=… attempt=… expected_hash=… attempted_hash=… reason=retry_crdt_merge`).

## Verification

- SimWorld: N concurrent-edit drift cycles (drift outside the compacted component)
  converge without duplicate archive files and land compacted content in HEAD.
- SimWorld: drift *inside* the compacted component fails closed with the actionable
  message, no partial write, recoverable via pre-compact tag.
- `make check` green; `make tmux-ci` green.
- Live JB proof is `[operator-verify]`: Compact Exchange while typing a burst converges
  or fails closed cleanly, never exit-1 with a raw CPC error.

## Distinct from

- `#jbcompactcommit` — that is compact *succeeding* but leaving HEAD at pre-compact
  content (uncommitted summary). This is compact *refusing outright* (exit 1) under
  concurrent-edit hash drift.
