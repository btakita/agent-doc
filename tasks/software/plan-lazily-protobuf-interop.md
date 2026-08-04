# Plan: Protobuf interoperability for Lazily graph boundaries

## Decision

Add optional Protobuf support to Lazily as a generated, versioned encoding of a
canonical reactive-boundary algebra. Do not make Protobuf the semantic owner of
the reactive graph.

The motivating failure is cross-language adapter drift: JetBrains received an
exact changed range but widened it to a whole-buffer snapshot, while VS Code
retained the bounded splice. A shared generated message would make that
representational drift harder, but graph ownership, merge laws, invalidation,
latest-wins behavior, and effect legality still require one normative Lazily
specification plus executable conformance traces.

The intended boundary is:

```text
host event
  -> generated typed graph-input message
  -> controller-owned Lazily graph
  -> generated typed projection/effect message
  -> thin host projector
```

Canonical JSON remains the human-readable fixture and diagnostic form. Msgpack
remains supported by the existing interop matrix. Protobuf is an additional
negotiated codec and code-generation surface, not a replacement for either.

## Scope

The first schema family should cover graph-boundary facts that currently drift
between Rust, Kotlin, and TypeScript:

- graph, source, node, and stable cell identity;
- source generation, causal epoch, monotonic sequence, and correlation id;
- bounded text splices against a stable cell, including local offsets and the
  expected cell revision;
- keyed membership upsert/remove;
- editor or host surface observations such as visibility, focus, and layout;
- derived projection versions;
- effect intents, durable delivery identity, and receipts;
- capability and schema-version negotiation.

Domain schemas may extend that envelope. The base protocol must not use a
whole-document value as the ordinary mutation primitive. Full snapshots are
typed separately and admitted only for bootstrap, explicit recovery, or
checkpoint compaction.

## Semantic ownership

Protobuf defines message shape, field types, field numbers, and compatibility.
It does not define:

- when a Source invalidates a Computed;
- how dependencies are discovered or retired;
- how concurrent source mutations merge;
- when a stale generation is rejected;
- whether latest-wins coalescing is legal;
- when an Effect may run or retry;
- whether a receipt proves visibility, persistence, or both.

Those rules remain in `lazily-spec`, its formal models where applicable, and
cross-binding conformance fixtures. Generated bindings must not grow private
reducers that reinterpret the graph.

## Schema design

Create a small `lazily.proto` package with a versioned envelope and explicit
oneofs. Avoid a universal untyped property bag.

Candidate logical messages:

- `ProtocolEnvelope`
- `CapabilityHandshake`
- `GraphInput`
- `SourceValueSet`
- `KeyedMemberUpsert`
- `KeyedMemberRemove`
- `CellTextSplice`
- `GraphSnapshot`
- `DerivedProjection`
- `EffectIntent`
- `DeliveryReceipt`

`CellTextSplice` should carry a stable graph/document id, stable cell id,
expected cell revision, local offset and delete length, inserted UTF-8 text,
source generation, causal epoch, and sequence. When an adapter cannot resolve a
stable cell id, it may send a bounded document splice to the semantic owner for
resolution; it may not promote the surrounding document snapshot to operator
authority.

Reserve field numbers and enum values on removal. Reject unknown enum
discriminants at semantic admission even when a Protobuf runtime preserves or
skips the unknown wire field. Do not hash raw Protobuf bytes as logical
identity: map order and implementation-specific serialization can differ.
Content hashes must use the existing canonical logical representation.

## Relationship to fixture schemas

This work does not replace JSON Schema coverage for conformance fixtures or
assertion blocks. Proto3 unknown-field behavior is deliberately forward
compatible and therefore cannot supply the fail-closed unknown-key guarantees
tracked by `#lzassertionblockschema`.

Use JSON Schema for fixture authoring validation and Protobuf for negotiated
runtime messages. Generate both from one reviewed logical field ledger where
practical, and test that neither surface silently gains or loses a field.

## Phases

### 1. Protocol ledger and architecture test

- Inventory the existing Lazily JSON/msgpack frames and Agent Doc Rust/Kotlin/
  TypeScript adapter messages.
- Classify every field as identity, causality, payload, projection, effect, or
  receipt.
- Write the normative mutation algebra before writing `.proto` files.
- Add an architecture test proving full snapshots are not accepted on the
  ordinary local-mutation variant.

Exit criterion: one reviewed field ledger and executable semantic examples for
bounded cell splices, stale epochs, duplicate sequences, and bootstrap
snapshots.

### 2. Rust/Kotlin/TypeScript pilot

- Add canonical `.proto` sources under `lazily-spec`.
- Generate Rust, Kotlin, and TypeScript bindings reproducibly.
- Keep generated sources either checked in everywhere or generated everywhere;
  do not mix policies between bindings.
- Add thin conversions between generated messages and the existing native
  Lazily types.
- Pilot Agent Doc editor events: JetBrains and VS Code publish the same
  `CellTextSplice` trace, while Rust owns cell resolution and graph effects.

Exit criterion: the same recorded edit/focus/reload trace produces the same
logical graph inputs, derived projections, and receipts in all three runtimes.

### 3. Codec and compatibility conformance

- Extend the interop channel matrix with negotiated Protobuf profiles.
- Replay canonical JSON fixtures through JSON -> logical model -> Protobuf ->
  logical model and the reverse direction.
- Test old-reader/new-writer and new-reader/old-writer pairs.
- Mutation-test unknown enum values, removed fields, reused field numbers,
  duplicate delivery, stale generation, out-of-order sequence, and a cache
  reload incorrectly labeled as operator input.
- Compare canonical logical hashes, not encoded byte equality.

Exit criterion: compatibility failures are typed and fail closed at semantic
admission; supported additive fields round-trip without changing graph
behavior.

### 4. Binding rollout

- Publish the schema package and generator versions.
- Add capability negotiation so peers use Protobuf only when both advertise the
  same compatible protocol range.
- Roll the protocol through the remaining Lazily bindings in the same parity
  ledger used by other interop features.
- Retain canonical JSON diagnostics and a bounded downgrade path until every
  supported peer combination is covered.

Exit criterion: the coverage ledger names every binding and codec combination,
and no adapter contains a private whole-snapshot mutation fallback.

### 5. Operational hardening

- Bound message, nesting, repeated-field, and decompression sizes.
- Fuzz decoders and logical admission separately.
- Preserve causal metadata in logs without logging sensitive payload bodies.
- Benchmark generated-message allocation, splice throughput, and large
  bootstrap snapshots independently.
- Document schema evolution, reserved-number policy, and release ownership.

## Verification

The minimum cross-language trace suite must include:

1. Three incremental edits (`Temp` -> `Tempor` -> `Temporal`) resolve to one
   final cell without creating intermediate sibling cells.
2. Two edits in different cells remain two bounded splices rather than one
   document-wide replacement.
3. A clean cache reload advances the non-operator epoch and fences queued local
   splices.
4. A remote projection followed by native-library reload preserves the same
   graph generation and does not republish the visible buffer as local input.
5. Duplicate and reordered frames converge or fail according to the normative
   algebra identically in Rust, Kotlin, and TypeScript.
6. Bootstrap snapshot and ordinary mutation use distinct message variants and
   cannot be confused by an adapter.

## Non-goals

- Serializing executable closures or arbitrary in-memory graph objects.
- Replacing Lazily's dependency tracking with a remote graph runtime.
- Encoding editor-specific APIs in the base protocol.
- Treating generated types as proof of semantic parity.
- Replacing JSON Schema validation for conformance fixtures.
- Requiring Protobuf for in-process Lazily use.

## Initial deliverable

Land the protocol ledger, the bounded-splice schema, reproducible
Rust/Kotlin/TypeScript generation, and the six conformance traces. Keep the
feature experimental until Agent Doc demonstrates that JetBrains and VS Code
produce identical logical traces for typing, cache reload, remote projection,
native reload, focus, and layout.
