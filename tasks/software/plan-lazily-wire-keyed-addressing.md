# Plan: Wire-stable keyed addressing for collection entries (`#lzwirekey`)

**Status:** Scoped — plan-only this cycle (operator chose "plan only" for `#lzwirekey`; runtime `CellFamily`/`CellMap` landed as `#lzcellfamily`).
**Created:** 2026-06-25
**Session:** `tasks/software/lazily-rs.md`
**Depends on:** `#lzcellfamily` (DONE — `src/lazily-rs/src/cell_family.rs`)
**Reference impl:** `src/lazily-rs/src/ipc.rs` (`NodeSnapshot` L335, `DeltaOp::NodeAdd` L464, `NodeState`), `PROTOCOL.md` (§NodeId L25, §NodeSnapshot L124, §Delta `NodeAdd` L233)

## Problem

`NodeId` is an opaque, wire-stable `u64` "decoupled from language-internal
allocation IDs" (PROTOCOL.md §NodeId), but the protocol has **no key→NodeId
registry**. A `CellMap`/`CellFamily` entry is addressable internally by its key
(`"alice"`, `(row, col)`, an A1 ref), yet on the wire a peer can only refer to it
by the opaque NodeId it happened to receive. That breaks across a resync or
reconnect: the producer may re-mint a removed-then-readded entry under a *new*
NodeId, and a subscriber that asked for "entry `alice`" has no stable handle.

Concretely, the gaps are:

1. **No stable keyed addressing on the wire.** A peer that wants "subscribe to
   entry `alice`" must map the key→NodeId out-of-band; the mapping is lost on
   NodeId churn.
2. **No sub-path addressing for nested collections.** Falls out of (1) once
   entries carry a key/path — a nested `CellMap` inside a `CellMap` entry needs a
   path, not just a flat key.

## Design

Add an **optional** `key` field to the two wire ops that introduce a node, plus
a small key→NodeId index on each side. Keep it additive and backward compatible:
absent `key` ⇒ today's opaque-only behavior.

### Wire changes (`src/ipc.rs`, `PROTOCOL.md`)

- `NodeSnapshot { node, type_tag, state, key: Option<NodeKey> }` — new trailing
  field; serde default `None` so existing encoders/decoders round-trip.
- `DeltaOp::NodeAdd { node, type_tag, state, key: Option<NodeKey> }` — same.
- `NodeKey` = a newtype over a `String` **path** (segments joined by `/`, e.g.
  `scores/alice`, `sheet/A1`, `outer/k1/inner/k2`). A path (not a flat string)
  covers nested collections with no extra machinery — requirement (2) is just a
  multi-segment `NodeKey`. Define an explicit max length + segment count to bound
  attacker-controlled growth (mirror the existing IPC payload caps).
- Wire format: encode `key` as an optional field in the JSON / postcard / msgpack
  codecs. Postcard/msgpack: append as `Option<NodeKey>` at the end of the struct
  so older readers that stop early still parse (verify with the codec
  conformance tests in `tests/conformance/`).

### Producer side (snapshot/delta builders)

- Extend the snapshot/delta builder so a node minted from a `CellMap`/`CellFamily`
  entry carries its `NodeKey`. This needs the runtime to know the key when it
  emits the node — wire `CellMap` so each entry records its `NodeKey` and the IPC
  projection reads it. (The runtime↔IPC projection seam lives wherever
  `NodeSnapshot`s are currently produced — audit `distributed.rs` / `ipc.rs`
  producers; that producer audit is the first implementation step.)
- `NodeRemove` already carries the NodeId; the index removal is keyed off that.

### Consumer side (key registry)

- Maintain `key_index: HashMap<NodeKey, NodeId>` updated on `NodeAdd`/`Snapshot`
  (insert) and `NodeRemove` (remove). Survives NodeId churn: a removed-then-
  readded `alice` updates `key_index["scores/alice"]` to the new NodeId, so a
  subscription expressed as a key stays valid.
- Public surface: `fn node_for_key(&self, key: &NodeKey) -> Option<NodeId>` and
  the inverse `fn key_for_node(&self, node: NodeId) -> Option<&NodeKey>`.

## Phases

1. **Producer audit (read-only).** Enumerate every site that constructs a
   `NodeSnapshot` / `DeltaOp::NodeAdd` and confirm where an entry's `NodeKey`
   would be available. Output: the exact seam list. (Do this before touching the
   wire types — it determines whether the runtime can supply keys at all.)
2. **Wire types + codecs.** Add `key: Option<NodeKey>` + `NodeKey`, update all
   three codecs, extend `tests/conformance/` round-trip + a cross-version
   (old-reader/new-writer) compatibility case.
3. **Consumer key index.** Add `key_index` + `node_for_key`/`key_for_node` +
   tests for the churn case (remove + re-add under a new NodeId keeps the key
   resolvable).
4. **Producer wiring.** Thread `NodeKey` from `CellMap`/`CellFamily` entries
   through the projection so emitted nodes carry their key; integration test:
   mutate one entry, assert the delta names it by key.
5. **PROTOCOL.md + SPEC.md.** Document the optional field, the path grammar, the
   bounds, and the backward-compat guarantee; add a Lean/spec obligation if the
   key index participates in a stated invariant.

## Non-goals / open questions

- Not changing `NodeId` semantics — `key` is *additive* addressing, NodeId stays
  the volatile internal handle.
- Key uniqueness/collision policy across producers (single-producer mirror today;
  multi-writer distributed boundary needs a documented last-writer rule — tie
  into `#lzcrdtplane`).
- Whether `key` should also annotate `EdgeSnapshot` (probably not — edges are
  addressed by endpoint NodeIds, which the key index already resolves).
