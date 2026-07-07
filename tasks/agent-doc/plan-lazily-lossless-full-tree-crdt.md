# Plan: Lazily lossless full-tree CRDT

`#lzlosstree`

## Operator Ask

Create an overarching plan for a lossless full-document tree CRDT implemented
across:

- `src/lazily-spec`
- `src/lazily-formal`
- `src/lazily-rs`
- `src/lazily-kt`
- `src/lazily-js`

Then plan the rollout into `src/agent-doc`.

This plan is intentionally broader than `#lzfreecell`. The goal here is not a
semantic tree with a separate flat text CRDT fallback. The goal is a full
lossless concrete document tree whose leaves include token, trivia, raw, and
error nodes so the tree itself is the lossless wire authority.

## Core Claim

Agent-doc can eventually remove the separate flat text CRDT floor only if the
new tree is a lossless concrete syntax tree, not a semantic AST.

Required invariant:

```text
render(tree) == original_document_text
```

for valid documents, invalid intermediate documents, unknown syntax, comments,
whitespace, duplicated markers, partial fences, broken list indentation, editor
newline normalization, and all raw spans that the parser cannot classify.

If this invariant is not true, the design still needs a flat lossless text floor.

## Target Architecture

### Tree Shape

The CRDT document is a single rooted tree:

- `DocumentRoot`
- `ElementNode`
  - semantic node with `kind`, attributes, and ordered children.
- `TokenNode`
  - exact source token text, for delimiters and syntax markers.
- `TriviaNode`
  - exact whitespace, blank lines, comments, indentation, and separators.
- `RawNode`
  - valid text that the adapter deliberately keeps opaque.
- `ErrorNode`
  - invalid or ambiguous text span that must still round-trip exactly.

All rendered source text belongs to exactly one live leaf node. Internal semantic
nodes own structure, not hidden text.

### CRDT Subsystems

- Node identity:
  - stable `TreeNodeId`, minted from a globally unique CRDT op id.
- Child order:
  - move-aware sequence CRDT for each parent child list.
- Leaf payloads:
  - per-leaf text CRDT for token/trivia/raw/error text.
- Node metadata:
  - LWW or MV registers for kind, parser version, attributes, confidence, and
    adapter-owned annotations.
- Tombstones:
  - structural and leaf tombstones retained until causal stability permits GC.
- Operation log:
  - dotted op ids or an equivalent non-contiguous frontier. A plain per-peer
    max counter must not be used if it can skip causally missing tree ops.

### Wire Model

The wire protocol sends tree deltas, not whole-document text:

- create node;
- delete/tombstone node;
- move node within a parent or across parents;
- update metadata register;
- edit leaf text;
- split leaf;
- merge adjacent leaves;
- replace subtree with a projection-preserving tree;
- checkpoint/snapshot a subtree after causal stability.

Every update carries:

- operation id;
- causal dependencies or dotted version information;
- affected node ids;
- parser/adapter capability version when syntax-specific;
- rendered-text hash of the affected subtree when useful for validation.

## Non-Negotiable Invariants

- `render(parse_lossless(text)) == text`.
- `render(apply(update, tree))` never drops bytes/string content.
- Every live non-root node has exactly one live parent.
- A live child id appears at most once in a parent order.
- The tree is acyclic.
- Token/trivia/raw/error leaf ordering is deterministic after concurrent edits.
- Unknown or invalid syntax is represented as `RawNode` or `ErrorNode`, never
  discarded.
- Semantic conflicts may degrade to raw/error nodes, but text preservation wins
  over semantic shape.
- Delta sync is idempotent, commutative under causal buffering, and complete for
  non-contiguous operation delivery.

## lazily-spec Workstream

### Specs

Add a new spec chapter:

- `docs/lossless-tree-crdt.md`
- link from `docs/SUMMARY.md`
- update `docs/protocol.md`
- update `docs/conformance.md`
- update `docs/coverage.md`

Define:

- tree state data model;
- node id and op id formats;
- node variants;
- child-order CRDT;
- leaf text CRDT embedding;
- operation envelope;
- state vector / dotted frontier format;
- snapshot and delta wire shapes;
- parser adapter contract;
- capability name: `lossless_tree_crdt_v1`.

### Schemas

Add schemas:

- `schemas/lossless-tree.json`
- `schemas/lossless-tree-delta.json`
- `schemas/lossless-tree-snapshot.json`
- extend `schemas/distributed.json` if anti-entropy frames carry the new ops.

### Conformance Fixtures

Add fixtures under `conformance/collections/` or
`conformance/lossless-tree/`:

- exact parse/render roundtrip;
- invalid markdown-like source roundtrip;
- token/trivia preservation;
- one-leaf text edit delta;
- semantic node metadata edit;
- split leaf and merge adjacent leaves;
- concurrent insert into the same parent;
- concurrent move and text edit;
- incompatible semantic shapes degrade to raw/error with preserved text;
- subtree snapshot compaction;
- anti-entropy replay with intentionally non-contiguous ops.

Each fixture should include:

- initial tree;
- operation schedule;
- expected rendered text;
- expected live node summary;
- expected convergence across at least two delivery orders.

## lazily-formal Workstream

### New Lean Modules

Add:

- `LazilyFormal/LosslessTree.lean`
- `LazilyFormal/LosslessTreeSync.lean`
- optionally `LazilyFormal/LosslessTreeProjection.lean` if projection proofs get
  large.

Import/reuse:

- `TextCrdt.lean`
- `TextCrdtSync.lean`
- `SeqCrdt.lean`
- `SemTree.lean`
- `Tree.lean`

### Proof Targets

Model the smallest syntax-agnostic core first:

- ordered tree with leaf text projections;
- `render` as concatenation of live leaves;
- node create/delete/move;
- leaf edit;
- split/merge leaf;
- subtree replace guarded by equal rendered text.

Prove:

- render determinism;
- parse adapter exactness as an assumption/interface theorem;
- node-local edits preserve unrelated subtree rendering;
- split and merge preserve full rendering;
- subtree replace preserves full rendering when guard passes;
- concurrent compatible operations converge;
- incompatible structural conflicts degrade to raw/error while preserving render;
- op apply is idempotent;
- merge/apply is commutative and associative under causal closure;
- sync frontiers cannot skip missing non-contiguous ops.

The formal model should explicitly reject any version-vector shortcut that
proves insufficient for tree ops.

## lazily-rs Workstream

### Modules

Add:

- `src/lossless_tree_crdt.rs`
- optionally `src/lossless_tree_adapter.rs`
- tests under `tests/lossless_tree*.rs`

Export from `src/lib.rs` after the API is stable enough for downstream use.

### Rust Types

Implement:

- `LosslessTreeCrdt`
- `TreeNodeId`
- `TreeOpId`
- `TreeVersionFrontier`
- `LosslessTreeNode`
- `NodeKind`
- `NodeAttrs`
- `LeafKind`
- `TreeUpdate`
- `TreeSnapshot`
- `LosslessParserAdapter`
- `ProjectionProof`

Use existing primitives where possible:

- `TextCrdt` for leaf text;
- `SeqCrdt` or a move-aware child order primitive for child lists;
- LWW/MV registers from the distributed CRDT layer for metadata;
- existing IPC/schema validation paths for encoded updates.

### Rust API Sketch

```rust
pub struct LosslessTreeCrdt { /* ... */ }

impl LosslessTreeCrdt {
    pub fn new(peer: u64) -> Self;
    pub fn from_lossless_tree(peer: u64, snapshot: TreeSnapshot) -> Result<Self>;
    pub fn render(&self) -> String;
    pub fn edit_leaf(&mut self, node: TreeNodeId, offset: TextOffset, delete_len: u32, insert: &str) -> Result<()>;
    pub fn create_node(&mut self, parent: TreeNodeId, after: Option<TreeNodeId>, node: LosslessTreeNode) -> Result<TreeNodeId>;
    pub fn move_node(&mut self, node: TreeNodeId, new_parent: TreeNodeId, after: Option<TreeNodeId>) -> Result<()>;
    pub fn tombstone_node(&mut self, node: TreeNodeId) -> Result<()>;
    pub fn split_leaf(&mut self, node: TreeNodeId, at: TextOffset) -> Result<TreeNodeId>;
    pub fn merge_adjacent_leaves(&mut self, left: TreeNodeId, right: TreeNodeId) -> Result<()>;
    pub fn replace_subtree_preserving_render(&mut self, root: TreeNodeId, replacement: TreeSnapshot) -> Result<()>;
    pub fn frontier(&self) -> TreeVersionFrontier;
    pub fn diff(&self, their_frontier: &TreeVersionFrontier) -> Result<TreeUpdate>;
    pub fn apply_update(&mut self, update: &TreeUpdate) -> Result<ApplyOutcome>;
}
```

### Offset Policy

The wire protocol should use UTF-8 byte offsets for source spans because
agent-doc documents are UTF-8 files and parser spans are byte ranges. Each
implementation must provide conversion helpers for host string models:

- Rust: UTF-8 string byte offsets with char-boundary validation.
- Kotlin: convert UTF-8 byte spans to/from JVM UTF-16 indices safely.
- JS: convert UTF-8 byte spans to/from UTF-16 string indices safely.

No implementation may silently treat UTF-16 code units as wire offsets.

### Rust Tests

- Replay every lazily-spec fixture.
- Property-test random structural/text schedules.
- Fuzz parser adapter projection proof with invalid source spans.
- Test causal buffering of out-of-order ops.
- Benchmark:
  - one-leaf edit in 45 KB / 250 KB / 1 MB docs;
  - response append in an agent-doc-like tree;
  - backlog item reap;
  - replay after restart;
  - projection checkpoint size.

## lazily-kt Workstream

### Modules

Add:

- `src/main/kotlin/io/github/lazily/LosslessTreeCrdt.kt`
- tests under `src/test/kotlin/io/github/lazily/LosslessTreeCrdtTest.kt`
- conformance replay tests under the existing conformance test style.

### Kotlin Requirements

- Mirror lazily-spec schema types.
- Preserve wire UTF-8 byte offsets; never use raw Kotlin `String` indices as
  protocol offsets without conversion.
- Match Rust op ordering and deterministic conflict resolution.
- Replay shared fixtures copied from `lazily-spec`.
- Add capability negotiation for `lossless_tree_crdt_v1` where the distributed
  plane exposes capabilities.

### Kotlin Verification

- Existing `./gradlew test` or Makefile equivalent.
- New conformance replay tests.
- Cross-check rendered output against Rust fixture expected text.

## lazily-js Workstream

### Modules

Add:

- `src/lossless-tree-crdt.js`
- `src/lossless-tree-crdt.d.ts`
- tests under `test/lossless-tree-crdt.test.js`
- schema/conformance replay under existing JS conformance tests.

### JS Requirements

- Explicit UTF-8 byte offset conversion helpers using `TextEncoder` /
  `TextDecoder`.
- Deterministic op id ordering identical to Rust/Kotlin.
- No reliance on UTF-16 code-unit positions in wire-visible APIs.
- Types exported from `src/index.js` and `src/index.d.ts`.
- Fixture replay for every lazily-spec tree fixture.

### JS Verification

- `npm test` / repository Makefile target.
- Schema validation tests.
- Property tests if existing JS test stack supports them; otherwise randomized
  deterministic test cases with seeded schedules.

## Cross-Repo Sequencing

1. `lazily-spec`: define vocabulary, schemas, capability, and first fixtures.
2. `lazily-formal`: prove the abstract projection and convergence core.
3. `lazily-rs`: implement reference runtime and generate additional fixtures.
4. `lazily-spec`: promote generated/reference fixtures into shared conformance.
5. `lazily-kt`: implement fixture-compatible runtime.
6. `lazily-js`: implement fixture-compatible runtime.
7. Update coverage matrices in `lazily-spec` and language READMEs.
8. Add release notes and version/capability gates.

Rust should lead implementation because agent-doc depends on it, but no
downstream rollout should treat the feature as protocol-stable until Kotlin and
JS replay the same conformance corpus.

## Agent-Doc Rollout Plan

### Rollout status

- **Phase 0 — DONE** (`dc3268ae`): `agent-doc-markdown-lossless` leaf crate parses a
  session document into a lazily `LosslessTreeCrdt` whose leaves own every byte, so
  `render(parse(doc)) == doc` by construction. `shadow_audit()` builds/renders/compares.
- **Phase 1 — shadow projection wired (`d1f4dfbd`)**: `shadow_audit` runs at the two
  hot-path current-document read chokepoints — preflight `run_with_options` (non-probe,
  `source=preflight_initial`) and `session-check` guard sweep (`source=session_check_guard_sweep`)
  — logging a stable `lossless_shadow …` line to `ops.log`. Pure/non-blocking; the flat
  CRDT stays the authority. Remaining Phase 1: collect live-session mismatch evidence
  (`match=false` lines) and haiven-sized/large-synthetic-doc build/render cost data before
  the exit gate is met. Verified in an isolated HEAD worktree because the main tree carried
  an unrelated concurrent session's `rpc.rs` WIP.
- **Phase 2 — first primitive landed (`3f9ce8b8`)**: `replace_component_inner(doc, name,
  new_inner)` expresses an agent-owned component mutation (status update, queue item edit)
  as a bounded lossless-tree edit — only the target component's body leaf changes, every
  other byte is identical by construction. Declines (→ legacy fallback) on missing or
  opaque/malformed components so it never eats marker bytes. Fully verified in the leaf
  crate (8 tests, clippy+fmt clean). **Not yet wired into the live write path.** Gate
  before wiring: byte-parity with `template::apply_patches` replace mode, plus a full
  `make check` — currently blocked because the working tree carries an unrelated
  concurrent session's controller/write-path (`rpc.rs`) WIP that fails the workspace build.
- **Phase 2 — first live strangler shadow landed (`005e79d2`)**: `write --status` now
  computes its component-body replace through `replace_component_inner` *alongside* the
  legacy `replace_status_content` projection and logs `lossless_write_parity op=status_set
  result=match|mismatch|declined` on every status write. Measurement only — legacy stays
  authoritative; the flip is a one-line swap once live parity is green. A parity test
  proves the tree result is byte-identical to legacy for the standard status shape
  (multi-line, multibyte, empty), fixing the inner mapping to `\n{text}\n` (element's
  `open_end` includes the newline after `-->`). `agent-doc-status-io`'s dep graph is clean
  of the contaminated `controller-io`, so it builds+tests in the main tree; deeper write
  paths (exchange append, backlog reap, queue edit) are the next strangler targets, then
  the merge/relay-layer replacement of the flat `CrdtDoc`/`MultiNodeState` — which lives in
  the contaminated hot path and is gated on the tree clearing + operator live-proof.
- **Phase 2 remaining / Phase 3 — agent-doable behind `make check` + haiven replay** once
  the tree clears: more agent-owned operations (append response, move boundary, consume
  backlog item, queue add/done), the before/after render guards against current visible
  text, and durable tree-projection persistence.
- **Phase 2 — proven drop-in for the projection primitive (`8156e44d`)**: a cross-check
  test against the real `agent-doc-element` parser proves `replace_component_inner` is
  byte-identical to `element::replace_content` for every clean-marker component (frontmatter,
  multi-component, multibyte, empty-inner, no-newline), declining to a legacy fallback only
  on malformed/opaque spans. `element::replace_content` is the primitive under essentially
  every component-body write that feeds the full-document CRDT, so the tree is now a
  wholesale drop-in at the projection layer, not a per-path shadow.

### Merge/relay-layer strategy (the actual "full document CRDT")

The `#lzlosstree` goal targets the flat `agent_doc_merge::crdt::{CrdtDoc, MultiNodeState}` +
`merge`/`merge_by_component` (yrs 3-way merge). Key finding while scoping this: **there is
already a parallel per-cell merge seam** — `merge_by_component` calls `try_cell_merge_text`
(the lazily `document_cell` `reconcile`-based per-cell 3-way merge, `#qcellmerge1`, default
ON) *before* the yrs whole-doc path. So the merge layer has two tree-ish models in play:
`document_cell::DocumentCellTree` (already the primary merge path) and the newer
`LosslessTreeCrdt` (`#lzlosstree`, currently Phase 0-2). The lossless-tree merge-layer
replacement must reconcile with, not ignore, the existing `document_cell` cutover.

**Merge-layer replacement STARTED (`e274f191`).** `merge_via_lossless_tree(base, ours,
theirs)` is a conservative per-component 3-way merge over the lossless tree: it requires
identical framing across all three sides, merges each component body with the standard
3-way rule, and returns `None` on conflict / structural or frontmatter divergence / opaque
markers so the authoritative CRDT keeps those. `merge_by_component` is now a thin wrapper
over the unchanged authoritative yrs implementation (`merge_by_component_authoritative`)
that ALSO runs the tree merge and logs `lossless_merge_parity result=match|mismatch|declined`
— pure measurement, the tree result is never returned, merge outcome unchanged.

Verified properly (not hand-reasoned): a byte-parity test cross-checks the tree merge against
the real `merge_by_component` on clean disjoint/one-sided cases; conflict/reframe/frontmatter
decline to `None`. The isolated-worktree gate passed in full — **7305 workspace tests green,
`cargo clippy --workspace -- -D warnings` clean** — resolving the earlier "can't full
`make check`" blocker by verifying at clean HEAD in the worktree rather than the contaminated
main tree. The markdown-lossless↔merge dev-dependency cycle (test-only) is allowed.

**Authority flip LANDED (`ebbc2840`).** `merge_by_component` now returns the lossless-tree
merge result as the authority-of-record whenever a structural 3-way is provably sufficient:
when `merge_via_lossless_tree` is confident AND byte-agrees with the causality-aware CRDT,
the tree's result is returned. Default-ON with an `AGENT_DOC_LOSSLESS_MERGE` kill-switch
(mirrors `#qcellmerge1`).

Key finding from the flip (the full-suite gate earned its keep): a naive flip — return the
tree result and skip the CRDT — **fails 4 tests**, all *stale-side causality* cases (3 queue
struck-head "stale unstrike must not resurrect", 1 exchange "never drop committed block on
stale side"). A pure-text tree merge is **causality-blind**: it cannot tell a forward edit
from a stale revert, so it regresses content the CRDT's op history protects. Hence the flip
is *guarded* — the tree wins only on byte-agreement; on divergence the CRDT result wins and
the case is logged `lossless_merge_divergence`. This is byte-transparent today (safe to
default ON) and the divergence log is the concrete worklist for the next phase.

**Phase 3 durable projection LANDED (`88a1a04b`).** `agent-doc-markdown-lossless` now exposes
`LosslessProjection { ops: TreeUpdate (full op-stream snapshot), rendered_sha256, rendered_len }`
with `project(doc)` / `restore(projection)` / `is_current_for(visible_text)` /
`projection_to_bytes|from_bytes`. The tree already carries op causality (dotted frontier +
op-log); this serializes it durably so current text can be rebuilt from the tree alone and a
stale projection (hash mismatch) can never override the editor-visible document. Verified:
7309 workspace tests, corpus + serde round-trip + staleness guard.

**FULL FLIP on the additive path LANDED (`24ce97be`).** `merge_by_component` now makes the
lossless tree the **sole authority — the flat full-document CRDT is not computed at all** —
whenever the tree is confident AND both sides only *inserted* relative to base (each a
line-supersequence of base: no deletion/reorder). With no deletion there is no stale-side
revert, so a structural 3-way is provably causally safe. This covers the common,
perf-critical case (appending an agent response, a new queue line): merge cost drops from a
whole-document yrs merge to the edited subtree. Verified: 7309 workspace tests pass with yrs
skipped on every additive merge in the suite (byte-correctness proof), clippy clean.

**Remaining for total CRDT elimination (the deletion path).** When a side *deleted* content, a
stale-side revert is possible, so the CRDT still runs as the guarded arbiter (tree wins on
byte-agreement, else CRDT; divergence logged). Making the tree authoritative there too needs
op causality from **editor-sent tree ops** (Phase 5) rather than reconstructed text — a stale
editor buffer would then arrive with a frontier that proves it is behind, instead of as
ambiguous text. That is the editor-capability + live-authority work (Phases 4-6). Also still
ahead: reconcile with the parallel `document_cell` (`#qcellmerge1`) per-cell path.

- **Phase 4 (Rust/FFI half) — agent-doable**: the `lossless_tree_crdt_v1` capability constant
  and negotiation logic in Rust/FFI. The JetBrains (lazily-kt) and VS Code (lazily-js) plugin
  halves plus the live-authority proof (Phases 5-6) remain `[operator-verify]` by the
  project's release rules and cannot be closed autonomously.

### Phase 0: Adapter Design

Create an agent-doc lossless Markdown/document adapter outside lazily. It should
emit a lossless tree with:

- frontmatter nodes;
- exchange prompt nodes;
- exchange response nodes;
- backlog/review/done/icebox item nodes;
- queue item nodes;
- status/component nodes;
- boundary/comment token nodes;
- trivia nodes for blank lines and indentation;
- raw/error nodes for unknown or invalid regions.

The adapter must prove:

```text
render(adapter.parse(doc_text)) == doc_text
```

for current session docs, compacted docs, corrupted docs, and invalid
intermediate editor buffers.

### Phase 1: Shadow Projection

Run the lossless tree as a shadow projection only:

- current flat text/live relay remains authoritative;
- every preflight/session-check reads current text, builds the tree, renders it,
  and checks exact equality;
- failures are logged with raw/error spans, not used for closeout decisions;
- collect performance data on haiven-sized docs and larger synthetic docs.

Exit gate:

- no projection mismatch across representative live sessions;
- parser produces bounded raw/error spans for invalid docs;
- shadow build/render cost is acceptable.

### Phase 2: Agent-Owned Tree Mutations

Route safe agent-owned operations through tree mutations, then render back to
text and apply through the current authority path:

- append response;
- move boundary marker;
- consume/reap backlog item;
- update status block;
- queue item add/edit/done;
- closeout recovery of known response blocks.

Each operation must verify:

- expected-current tree render matches current visible text before mutation;
- rendered result is exactly the intended document text after mutation;
- fallback to legacy path if the touched region is raw/error or confidence is
  below threshold.

Exit gate:

- existing agent-doc `make check`;
- plugin tests;
- haiven replay with no duplicate prompt/response materialization;
- no full-document replacement required for the routed operations.

### Phase 3: Durable Tree Projection

Persist a tree snapshot/update stream as a durable projection:

- state-backbone records tree projection version and rendered text hash;
- commit barrier records tree frontier and visible text hash;
- recovery can rebuild current text from the tree projection;
- stale projection never overrides editor-visible text without frontier/hash
  proof.

Exit gate:

- crash/restart tests prove projection replay;
- stale/corrupt projection degrades to rebuilding from visible text;
- no sidecar can become hotter authority than the visible document.

### Phase 4: Editor Plugin Capability

Add capability negotiation:

- `lossless_tree_crdt_v1` in Rust/FFI;
- JetBrains plugin support via lazily-kt;
- VS Code/plugin JS support via lazily-js;
- explicit fallback when a plugin lacks the capability.

The editor protocol should send:

- tree update frames;
- visible rendered-text hash receipts;
- parser adapter version;
- raw/error span diagnostics when the editor buffer is invalid.

Exit gate:

- maintained plugins replay shared fixtures;
- editor receipts prove rendered text, not just accepted patches;
- unsupported plugin versions fail closed with upgrade guidance.

### Phase 5: Tree As Live Authority

For capable plugins only, make the lossless tree the live authority:

- editor typing updates leaf nodes or raw/error nodes;
- invalid intermediate Markdown remains in tree error/raw leaves;
- agent-owned operations update semantic nodes;
- rendered text is flushed to disk/git at commit boundaries;
- flat text CRDT relay remains compatibility fallback for older plugins.

Exit gate:

- live typing, response append, backlog reap, compact, reset, and closeout
  recovery all converge through tree authority;
- haiven-sized live session no longer produces pathological projection growth;
- controller RPC paths do not do whole-document CRDT replacement work.

### Phase 6: Deprecate Flat Relay For Capable Sessions

After sustained evidence:

- stop creating flat CRDT projections for `lossless_tree_crdt_v1` sessions;
- keep import/export tools for legacy recovery;
- retain flat text path only for detached or old-plugin compatibility;
- update runbooks and troubleshooting around tree projection/frontier health.

## Performance Goals

Initial targets, to be refined by benchmarks:

- one backlog item edit sends O(item) update, not O(document);
- response append sends O(response) update plus small parent-order metadata;
- invalid syntax near one region affects only that raw/error subtree;
- restart replay for a 45 KB agent-doc document completes under the controller
  RPC timeout budget with margin;
- projection size grows with edited nodes and tombstone retention policy, not
  with repeated whole-document replacements.

## Verification Matrix

### Required Before Library Feature Is Stable

- `make check` in `src/lazily-spec`.
- `make check` in `src/lazily-formal`.
- `make check` in `src/lazily-rs`.
- Kotlin test suite in `src/lazily-kt`.
- JS test suite in `src/lazily-js`.
- Conformance fixtures replayed by Rust, Kotlin, and JS.
- Schema validation for snapshots/deltas.
- Benchmarks checked against documented budgets.

### Required Before Agent-Doc Live Rollout

- `make check` in `src/agent-doc`.
- JetBrains plugin test suite.
- VS Code plugin test suite.
- Live-session replay for haiven-sized docs.
- Corrupt/intermediate Markdown replay corpus.
- Session-check/preflight closeout tests using tree projection.
- Controller recycle/restart tests proving no wedged handoff on tree projection
  rebuild.

## Risk Register

- A semantic AST is accidentally treated as lossless. Mitigation: token/trivia
  and raw/error nodes are mandatory in schemas and fixtures.
- UTF-16/UTF-8 offset drift across Kotlin/JS. Mitigation: byte-offset fixtures
  with non-ASCII text and invalid edit boundaries.
- Version vectors skip non-contiguous tree ops. Mitigation: formal proof and
  conformance schedule requiring causal gaps.
- Parser adapters disagree across versions. Mitigation: adapter version in
  operation metadata; incompatible shapes degrade to raw/error while preserving
  text.
- Memory overhead exceeds flat CRDT for small docs. Mitigation: benchmarks and
  compact subtree checkpoints.
- Agent-doc switches authority too early. Mitigation: shadow projection and
  tree-as-live-authority only after plugin capability proof.

## Open Questions

- Should `LosslessTreeCrdt` live in the main lazily package immediately, or in a
  feature-gated/companion module until protocol stability?
- Is the child-order CRDT a direct `SeqCrdt` reuse or a tree-specific wrapper
  with parent-aware invariants?
- Do token and trivia leaves need separate node variants, or can both be typed
  `LeafNode` metadata?
- How much parser adapter API belongs in lazily versus agent-doc?
- What is the first supported syntax adapter: agent-doc Markdown only, or a
  generic fixture adapter used for conformance?

## Success Criteria

- The tree itself is lossless: no separate flat CRDT floor is needed for capable
  sessions.
- Rust, Kotlin, and JS converge on the same shared fixtures.
- Lean proves the core render-preservation and convergence invariants.
- Agent-doc can update semantic regions without whole-document CRDT waves.
- Invalid intermediate Markdown remains editable and transmissible as tree
  raw/error nodes.
- Live agent-doc sessions can recover, preflight, finalize, and session-check
  through the tree authority without stale sidecar or projection starvation.
