# Plan: Exchange document-tree (per-node CRDT), lazily seqcrdt migration, exchange API, IPC-action unification

## Why

Four related problems, one root: the exchange is merged as **one opaque text blob**
(whole-doc yrs `Y.Text`), so structurally-distinct things (separate responses, a
user prompt) can **bleed into each other** on a 3-way / CRDT merge, and there is no
**structural API** to manipulate exchange entries — mutating the exchange means raw
text edits + `reset --from-current` (as done to clean the reitrades IPC-diagnostic
pollution).

1. **Cross-response merge bleed** — two `### Re:` responses (or a response and an
   adjacent prompt) can merge into each other because the CRDT sees one text field,
   not a list of nodes. This is the deeper fix behind the component-merge scramble
   work (`#qcellmerge1`).
2. **No structural exchange API** — removing the two `### Re: IPC proof diagnostic`
   blocks from `tasks/recruit/reitrades.md` required a raw file edit + snapshot/CRDT
   re-baseline. There should be an API call for that.
3. **User prompts are not distinct from responses** — a prompt in the exchange gets
   associated with a response's text region and can be swept into it.
4. **IPC action sprawl** — `source=` / `invariant=` / `recovery=` are ad-hoc string
   literals repeated across `write/ipc.rs`, `transport.rs`, `converge.rs`.

## Grounding (verified)

- `lazily 0.13.1` is already a dep of `agent-doc-merge`. It exposes:
  - `lazily::SeqCrdt<Id, V>` (`seq_crdt.rs`): fractional-`Position` sequence CRDT —
    `insert_between/insert_back/insert_front`, `move_between/after/before`,
    `set_value`, `remove`, `contains`, `get`, `order() -> Vec<Id>`,
    `values() -> Vec<(Id, V)>`, `merge(&other)`, `gc(watermark)`, tombstones + HLC.
  - `lazily::TextCrdt` (`text_crdt.rs`): character-level text CRDT (`OpId`,
    `parse_blocks`) for intra-node content.
- Current `yrs::` footprint is **3 files** only:
  `agent-doc-merge/src/crdt.rs`, `agent-doc-merge/src/crdt_sync.rs`,
  `agent-doc-markdown-ast/src/crdt.rs`. Migration is contained.
- On-disk document format is **unchanged**: `<!-- agent:exchange -->`, `### Re:`
  headers, `<!-- agent:boundary:… -->` markers, prompts. The tree is the in-memory
  / CRDT representation the markers parse into; round-trip must be byte-stable.

## Phase 1 — Dogfood-scope gate + consolidation ✅ DONE

- `fix(preflight)`: gate `append_latest_ipc_dogfood_note` on
  `dogfood_agent_doc_crate_root(file).is_some()` so IPC diagnostics never fold into
  a non-dogfood user document (root cause of the reitrades pollution). Committed
  `eb73b5af`; consolidated to the single canonical helper in `20cea70d`.

## Phase 2 — Exchange document-tree model (the anti-bleed structure)

Model the exchange as a **two-level CRDT** instead of one text blob:

```
ExchangeTree = SeqCrdt<NodeId, ExchangeNode>          // ordered list of nodes
ExchangeNode = {
    kind: Prompt | Response { header: String, model: Option<String> },
    body: TextCrdt,                                    // per-node character merge
    boundary: Option<BoundaryId>,                      // (HEAD)/new-since markers
}
NodeId = stable id (content-derived + peer-unique), survives re-parse
```

- **Every `### Re:` response** = one `ExchangeNode { kind: Response, body }`.
- **Every user prompt** in the exchange = one `ExchangeNode { kind: Prompt, body }`,
  never merged into a neighboring response.
- **Merge isolation:** merging two `ExchangeTree`s merges the `SeqCrdt` order
  (fractional positions ⇒ no reorder bleed) and merges each node's `body` `TextCrdt`
  **only against the same NodeId**. A response can never absorb text from another
  response or a prompt — this is the structural guarantee the current whole-doc
  merge lacks.
- **Parser bridge:** `agent-doc-element` / `agent-doc-markdown-ast` gains
  `parse_exchange_nodes(exchange_str) -> Vec<ExchangeNode>` and
  `render_exchange_nodes(&tree) -> String`, byte-stable round-trip (headers,
  boundaries, blank-line spacing preserved). NodeId assignment is deterministic on
  re-parse so re-open ↔ CRDT ids stay stable.
- Tests: response-cannot-absorb-neighbor, prompt-stays-distinct, concurrent
  add-response on both sides both survive, boundary markers ride with their node.

## Phase 3 — Replace yrs with lazily seqcrdt across the system

Scope = the 3 yrs files + their callers. Order:

1. `agent-doc-markdown-ast/src/crdt.rs` — swap the yrs document model for
   `SeqCrdt<NodeId, ExchangeNode>` (structure) + `TextCrdt` (node bodies). Keep the
   public merge API shape so callers don't churn at once.
2. `agent-doc-merge/src/crdt.rs` — route `merge_by_component` / the component hot
   path through the seqcrdt tree for `exchange`; other components stay whole-text
   `TextCrdt` (they have no sub-node structure).
3. `agent-doc-merge/src/crdt_sync.rs` — port the dormant state-vector sync model to
   `SeqCrdt::merge` + `gc(watermark)` (lazily has HLC + tombstone GC natively).
4. Delete the `yrs` dependency once the 3 files are ported; update `Cargo.toml` +
   `VERSIONS.md`.
- **CRDT state file:** `.agent-doc/crdt/<hash>.yrs` → `.<hash>.seqcrdt` (lazily
  `IpcCodec` encode/decode); add a one-shot migration that rebuilds state from the
  current document on first open (safe — document text is authoritative).
- Back-compat gate: keep a read path for existing `.yrs` state during rollout, or
  rebuild-from-disk (preferred; simpler, and the document is the source of truth).

## Phase 4 — Exchange-manipulation API (what I should have called for reitrades)

Deterministic structural ops on `ExchangeTree`, exposed at all three layers
(binary-owned per the "all deterministic behavior in the binary" rule):

- **CLI** (`src/exchange.rs`, new): `agent-doc exchange`
  - `list <FILE>` — node ids, kind, header, first line (JSON).
  - `remove <FILE> --id <NodeId>[…]` — drop node(s) + re-baseline snapshot/CRDT
    (this is the reitrades cleanup as one call, no raw edit + `reset`).
  - `add-response <FILE> --header <H> [--model M] < body` / `add-prompt <FILE>`.
  - `move <FILE> --id <N> --after|--before <Anchor>`.
- **FFI** (`ffi.rs`): `agent_doc_exchange_nodes(file) -> json`,
  `agent_doc_exchange_remove(file, node_id) -> bool`, mirroring the CLI for editors.
- **lib** (`lib.rs`): the same as pure functions over `ExchangeTree` for tests +
  the merge core.
- All ops go through the CRDT tree + snapshot/CRDT re-baseline, so an editor buffer
  and headless path converge (no bleed, no manual reset).

## Phase 5 — IPC-action unification (after the concurrent guard-extraction settles)

Replace the ad-hoc `source=`/`invariant=`/`recovery=` string literals with one
typed projection (mirrors the `CyclePhase → TurnProjection` pattern):

```
enum IpcDeliverySource   { SocketAckContent, SocketAlreadyApplied, SocketAckMismatch, FileIpc, Disk }
enum IpcProofInvariant   { Ack, ResponseProbe, LivePromptDrift, PromptDuplication } // probe merges socket+disk
enum IpcRecovery         { RetryWithoutDiskWrite, VisibleRepair, ContentOursSnapshotRepair, BlockExternalDiskWrite }
struct IpcDeliveryOutcome { source, invariant: Option<IpcProofInvariant>, recovery }
```

- All of `write/ipc.rs`, `transport.rs`, `converge.rs` classify into one
  `IpcDeliveryOutcome`; the guards + `op_log` emit off the typed value, not literals.
- Merge `missing_response_probe` + `disk_missing_response_probe` into one
  `ResponseProbe` invariant parameterized by `source`.
- **Behavior-preserving:** the fail-closed branches (`retry_without_disk_write` vs
  `visible_repair`) are kept distinct — only the representation is unified.
- **Sequencing:** the concurrent session is actively extracting write/session/ipc
  guards (`extract write pending checks`, `extract ipc editor targeting io`, …).
  Build this projection **on top of** their extracted structure once green; do not
  race it.

## Risks / sequencing

- **Concurrent refactor:** Phase 5 and any `write/*.rs` change must wait for the
  in-flight guard extraction to land (it briefly left `write.rs` referencing a
  missing `pending_checks` module — clean checkouts didn't compile). Coordinate.
- **Round-trip stability is the acceptance gate** for Phases 2–3: parse → render must
  be byte-identical on the full existing corpus before deleting yrs.
- **Migration is rebuild-from-disk**, not state translation — document text is
  authoritative, so first-open rebuilds `.seqcrdt` from the markers. No data at risk.
- Ship Phases 2→3→4 in order (tree model → migrate merge onto it → expose API);
  Phase 5 independently, last.
```
