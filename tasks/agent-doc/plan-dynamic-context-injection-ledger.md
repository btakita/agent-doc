# Plan: agent-doc dynamic context injection ledger

**Goal:** let agent-doc use tsift dynamic context packs without adding duplicate
context to an agent turn. agent-doc records what was injected for each document
cycle/session and decides whether to expand a pack, reference it by handle, or
skip it.

## Boundary

agent-doc owns workflow-local context policy:

- active document id, session id, cycle id, harness, and prompt boundary
- what context was injected into the current turn/session
- when compact/restart clears or preserves the injection ledger
- how preflight/plan/orchestrate attach tsift context packs
- the live reactive projection from document components and tsift pack metadata
  into the final prompt-context manifest

tsift owns reusable pack generation and source-level cache freshness. agent-doc
should call tsift and persist only pack/chunk usage, not duplicate tsift's chunk
store.

Use `lazily-rs` for the live in-process derivation layer, not for durable
storage. SQLite remains the ledger. Lazily cells/slots model the current document
components, preflight/plan fields, candidate context packs, and already-injected
sets so prompt assembly only recomputes when an input hash, component, or ledger
row changes.

## Storage

Add context usage rows in `.agent-doc/state.db` or a focused
`.agent-doc/context.db`:

- `context_injections(id, document_id, session_id, cycle_id, harness, pack_id, chunk_id, content_hash, source_uri, range_start, range_end, injected_at, injection_mode)`
- `context_manifest(cycle_id, prompt_fingerprint, pack_ids_json, chunk_ids_json, token_count, created_at)`

`injection_mode` examples: `expanded`, `referenced`, `skipped_duplicate`,
`stale_ignored`.

## Phases

### Phase 1 - `#adctxreactive`: lazily-backed live context graph

Add an agent-doc context projection module backed by `lazily-rs`:

- Cells: document component hashes, active queue/backlog/review heads,
  preflight/plan output, tsift pack metadata, injection ledger snapshot.
- Computed slots: prompt targets, candidate context chunks, duplicate/suppressed
  chunks, rendered context manifest.
- Memo guards: suppress downstream recompute when equal prompt targets or equal
  chunk sets are derived from unchanged inputs.

This module should be pure and testable. It should not perform SQLite writes or
call tsift directly; callers feed it snapshots and consume the derived manifest.

Tests:
- unchanged component hashes do not recompute the rendered context manifest
- changing a candidate chunk hash recomputes only affected duplicate decisions
- equal derived prompt targets preserve the memoized manifest

### Phase 2 - `#adctxledger`: durable injection ledger

Create the schema and small library helpers:

- `record_context_manifest(cycle_id, packs, chunks)`
- `already_injected(session_id/document_id, chunk_id/content_hash)`
- `clear_context_scope(scope)` for explicit session clear/compact boundaries

Tests:
- same chunk id is detected as already injected
- same content hash from another pack is detected as duplicate
- a new cycle records a new manifest without mutating prior cycles

### Phase 3 - `#adctxpreflight`: preflight/plan context manifests

When `agent-doc plan` or prompt assembly requests dynamic context, call tsift's
context-pack API and attach pack metadata to the plan output. Before prompt
assembly, feed pack metadata and ledger rows into the lazily projection. Render
repeated context as a compact handle reference instead of expanding duplicate
text.

Tests:
- first use expands pack text and records chunk ids
- second use in the same session references existing handles
- changed source hash expands the new chunk and records a new hash

### Phase 4 - `#adctxorchestrate`: child-agent and queue behavior

Thread context manifests through sequential/parallel/DAG orchestration so child
agents receive the minimum needed context. Worktree child agents should receive
pack handles and bounded excerpts, not full repeated session history.

Tests:
- parallel child prompts share pack handles without duplicating chunk text
- queue continuation does not re-expand unchanged context
- focused/supervisor restart keeps only durable pack handles needed for the next
  item

### Phase 5 - `#adctxcompact`: compact/restart semantics

Define which ledger scopes survive compaction, `/clear`, supervisor recycle, and
session transfer. Compact summaries should reference pack ids/source handles when
older context remains recoverable through tsift.

Tests:
- compact preserves enough manifest data to avoid immediate duplicate injection
- explicit session clear can start a fresh injection scope when intended
- transfer/extract can carry context handles without copying full content

## Acceptance

- agent-doc never needs to replay a full document/transcript just to avoid
  duplicate dynamic context.
- Prompt assembly can prove why a chunk was expanded, referenced, or skipped.
- lazily-rs owns only live derivation; SQLite owns durable context usage.
- Session-check or diagnostics can print the context manifest for a cycle.
- `make check` passes in `src/agent-doc`; add unit tests around ledger dedupe and
  prompt-render decisions.
