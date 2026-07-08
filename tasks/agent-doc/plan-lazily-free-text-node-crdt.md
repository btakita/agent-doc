# Plan: Lazily free-text node CRDT

`#lzfreecell`

## Operator Ask

Design lazily support for a hybrid tree/text CRDT model where structured nodes
carry isolated per-node CRDT state and ambiguous or invalid source regions fall
back to free-text nodes. The immediate motivation is agent-doc: whole-document
text CRDT remains necessary for exact live editor convergence, but agent-owned
semantic operations should eventually update bounded cells instead of generating
whole-document replacement waves.

## Problem

The current flat `TextCrdt` gives a safe lossless merge floor for arbitrary
text, including invalid intermediate Markdown. It does not know about semantic
regions, so structured operations such as "append response", "reap backlog
item", or "update status" are represented as edits against one large sequence.
That has two costs:

- large whole-document replacements produce large deltas and tombstone sets;
- downstream applications cannot tell whether an edit touched one semantic item
  or the whole document without reparsing and comparing text.

A pure tree CRDT would improve locality, but it cannot safely replace flat text
for live editor input because invalid intermediate syntax can make node identity
ambiguous. The target model is therefore hybrid:

- tree/node CRDT for stable semantic structure;
- per-node `TextCrdt` for each editable text cell;
- free-text nodes for raw spans whose structure is unknown, invalid, or unsafe
  to assign to semantic cells;
- deterministic promotion/demotion between free-text nodes and semantic nodes
  when parsing becomes stable again.

## Design Principles

- Exact text projection is authoritative. A tree state must project to the exact
  source bytes/string it represents, including whitespace and invalid syntax.
- Parsing is a projection, not authority. Parser adapters may propose semantic
  nodes, but the CRDT core must not lose raw text when parsing is ambiguous.
- Node locality is a performance contract. A semantic operation that touches one
  node must not require rewriting unrelated node text CRDTs.
- Invalid syntax is contained. Broken Markdown, half-open fences, and partial
  list indentation should collapse only the affected span into a free-text node,
  not force a whole-document fallback.
- Promotion/demotion must be convergent. Two replicas that independently
  promote, demote, split, or merge the same raw region must converge without
  duplicated or lost text.
- Lazily stays syntax-agnostic. Markdown-specific classification belongs in
  agent-doc or a separate adapter crate; lazily should expose generic node,
  span, and parser-adapter contracts.

## Proposed Lazily Surface

### Core Types

- `FreeTextTreeCrdt`
  - owns a move-aware node order CRDT plus a map of node payload CRDTs.
  - projects to a single source string by concatenating node projections.
- `FreeTextNodeId`
  - stable identity for one node, minted by CRDT op id and preserved across
    compatible promotions/demotions.
- `FreeTextNode`
  - variants:
    - `Raw { text: TextCrdt }`
    - `Semantic { kind, attrs, fields }`
    - `Boundary { token }` only if explicit boundary nodes are needed to
      preserve formatting without attaching it to neighboring nodes.
- `SemanticField`
  - per-field `TextCrdt` or scalar CRDT value. Text fields handle prose,
    headings, list item bodies, and raw attribute strings.
- `TreePatch`
  - transport operation for node insert/remove/move, node kind changes, field
    updates, split/merge, promotion, and demotion.
- `ParserProjection`
  - adapter output: proposed semantic nodes, raw spans, confidence/stability
    flags, and source-span mapping.

### Operations

- `insert_raw(after, text)`
- `insert_semantic(after, kind, fields)`
- `edit_text(node_id, field_id, offset, delete_len, insert)`
- `split_raw(node_id, at_offset)`
- `merge_adjacent_raw(left_id, right_id)`
- `promote_raw(node_id, projection)`
- `demote_semantic(node_id, raw_text)`
- `replace_node_projection(node_id, expected_text, new_projection)`
- `project_text()`
- `diff(their_state_vector)` / `apply_update(update)`

The Rust API should support byte offsets at adapter boundaries and an explicit
internal text granularity choice. Existing `TextCrdt` is char-indexed; the plan
must decide whether the free-text tree exposes UTF-8 byte offsets, Unicode scalar
indices, or grapheme-aware indices, and then test every conversion boundary.

## Promotion And Demotion Model

### Demotion

When a semantic region becomes invalid or parser confidence drops, the affected
semantic node range is replaced by one raw node whose `TextCrdt` is seeded from
the exact projection of the demoted nodes. Demotion records provenance:

- source node ids;
- source text hashes;
- demotion op id;
- parser reason.

Provenance is advisory for future promotion and conflict diagnostics; raw text
remains the merge authority.

### Promotion

When a raw node parses into a stable semantic projection, promotion replaces the
raw node with semantic nodes whose concatenated projection equals the raw text.
Promotion is allowed only when:

- the adapter proves exact projection equality;
- all proposed source spans partition the raw node text with no gaps or overlap;
- ambiguous trivia has a deterministic owner or is represented as raw/boundary
  nodes;
- concurrent edits to the raw node are either included in the promoted text or
  force promotion retry.

### Concurrent Promotion/Demotion

The merge rule should prefer text preservation over semantic shape:

- If two semantic shapes project to the same text, choose a deterministic shape
  by op id / adapter version / stable-id score.
- If semantic shape conflicts but text can be preserved, demote the conflicting
  region to raw.
- If one side edits raw text while the other promotes, rebase the promotion only
  when span mapping still proves exact coverage; otherwise keep raw.

## Formal Model Plan

Lean should prove the small core first, before implementation details harden.

Phase A: existing foundations

- Reuse/prove assumptions for `TextCrdt` convergence and idempotent delta apply.
- Reuse/prove assumptions for move-aware node order convergence.

Phase B: free-text tree projection

- Define a tree state as ordered nodes plus per-node text projections.
- Prove deterministic `project_text`.
- Prove node-local text edits preserve unrelated node projections.
- Prove split+merge preserve `project_text`.

Phase C: promotion/demotion

- Model `promote_raw` as a projection-preserving refinement.
- Model `demote_range` as a projection-preserving coarsening.
- Prove promotion followed by demotion preserves text.
- Prove concurrent compatible promotions converge.
- Prove incompatible promotion conflicts demote to raw while preserving text.

Phase D: transport

- Prove op apply idempotence.
- Prove pairwise merge commutativity/associativity for tree ops plus per-node
  text ops.
- Prove state-vector sync cannot skip causally missing node or text ops. This
  should explicitly test the existing per-peer max-counter shortcut and replace
  it if the model requires dotted version vectors or op-set frontiers.

Lean proof does not replace implementation testing. It defines the reference
model and invariants; Rust and every other binding must replay generated
conformance fixtures from the model.

## Implementation Phases

### Phase 0: Spike And Scope

- Audit current `TextCrdt`, `SeqCrdt`, `CellTree`, `SemTree`, and distributed
  plane APIs.
- Decide whether the core belongs in `src/text_tree_crdt.rs`, `src/sem_tree.rs`,
  or a new `src/free_text_tree.rs`.
- Define non-goals for v1: no Markdown parser in lazily, no rich-text styling,
  no collaborative cursor semantics beyond existing awareness primitives.

### Phase 1: Spec And Fixtures

- Add lazily-spec schemas for free-text tree state, ops, updates, and projection
  fixtures.
- Add conformance cases:
  - raw edit convergence;
  - semantic node local edit;
  - split/merge raw preservation;
  - raw promotion;
  - semantic demotion;
  - concurrent promotion vs raw edit;
  - incompatible semantic shapes collapse to raw;
  - invalid syntax contained to one span.

### Phase 2: Rust Core

- Implement `FreeTextTreeCrdt` using existing lazily CRDT primitives.
- Add projection-preserving `split_raw`, `merge_raw`, `promote_raw`, and
  `demote_range` operations.
- Add delta sync with a frontier that cannot miss non-contiguous ops.
- Add tombstone/compaction policy for removed nodes and per-node text CRDTs.
- Add property tests for random edit/promote/demote sequences.

### Phase 3: Parser Adapter Contract

- Define a syntax-agnostic adapter trait:

```rust
trait FreeTextParserAdapter {
    type Kind;
    type Attrs;

    fn project(&self, text: &str) -> ParserProjection<Self::Kind, Self::Attrs>;
    fn render(&self, node: &FreeTextNode<Self::Kind, Self::Attrs>) -> String;
}
```

- Require adapters to provide exact source-span coverage and deterministic trivia
  ownership.
- Add a fake adapter test harness that simulates invalid intermediate syntax
  without depending on Markdown.

### Phase 4: Cross-Language And FFI

- Extend lazily-spec conformance fixtures.
- Implement Rust first, then Python/Kotlin/JS/etc. after the schema settles.
- Add C-ABI types only after the operation vocabulary is stable.
- Add capability negotiation so peers can explicitly advertise
  `free_text_tree_crdt_v1`.

### Phase 5: Agent-Doc Adapter

- Build an agent-doc Markdown adapter outside lazily.
- Map stable sections to semantic nodes:
  - frontmatter;
  - exchange responses;
  - backlog items;
  - done/review/icebox items;
  - status blocks;
  - queue prompts;
  - raw/free-text spans around invalid or unknown regions.
- Route agent-owned operations through semantic node ops when confidence is high.
- Keep flat document `TextCrdt` as the live editor source of truth until the
  adapter can prove a tree projection is exact and current.

### Phase 6: Performance And Compaction

- Benchmark against flat `TextCrdt` on 45 KB, 250 KB, and 1 MB markdown docs.
- Track:
  - update size for one-node edits;
  - projection size;
  - promotion/demotion cost;
  - replay time after restart;
  - tombstone growth after repeated response append/reap cycles.
- Add safe compaction:
  - per-node text snapshot when all peers observe tombstones;
  - raw-node coalescing for adjacent stable raw spans;
  - semantic node snapshotting after durable commit barriers.

## Agent-Doc Integration Strategy

Use the hybrid model as an optimization and semantic command layer, not as the
first live authority replacement.

1. Keep the existing flat `TextCrdt` relay as the lossless live buffer authority.
2. Maintain a lazily free-text tree projection from the current flat text.
3. Express agent-owned mutations as tree ops when projection confidence is high.
4. Render the tree op result back to text and verify exact expected-current
   equality before applying it to the flat relay.
5. If projection confidence drops, fall back to flat text CRDT and mark the
   affected span raw.
6. Only after long-running conformance evidence should agent-doc consider making
   the free-text tree the primary durable projection.

## Verification Gates

- `make check` in `src/lazily-rs`.
- Lean builds in `lazily-formal` and `lazily-spec/formal/lean`.
- Cross-language conformance fixture replay.
- Rust property tests for random edit/promote/demote schedules.
- Parser fuzzing with invalid Markdown-like inputs in the adapter crate.
- A replay test using real agent-doc session documents including haiven-sized
  documents and corrupted/intermediate Markdown samples.
- Benchmarks proving one-node edits avoid whole-document delta growth.

## Open Questions

- Should text offsets be UTF-8 bytes, Unicode scalar indices, or grapheme
  clusters at the public API boundary?
- Does the existing `TextCrdt` version vector need dotted version vectors before
  it can safely support out-of-order free-text tree updates?
- Should formatting trivia attach to neighboring semantic nodes, dedicated
  boundary nodes, or raw nodes?
- What is the deterministic conflict policy when two parser versions promote the
  same raw text into different but text-equivalent semantic shapes?
- How much of this belongs in `lazily` versus a `lazily-text-tree` companion crate
  until the API stabilizes?

## Success Criteria

- Invalid intermediate syntax can be represented without losing exact text.
- Semantic edits update isolated nodes and produce bounded updates.
- Promotion/demotion preserve projection text by construction.
- Concurrent raw edits and semantic operations converge.
- Lean proves the core projection/convergence invariants.
- Cross-language fixtures prevent implementation drift.
- Agent-doc can use the model for structured operations while preserving the
  flat live text CRDT as a fallback authority.
