# Plan: tsift dynamic context cache

**Goal:** make tsift the reusable dynamic-context substrate: source chunks,
summaries, context packs, and dedupe metadata are cached in SQLite so agents can
ask for bounded context without repeatedly injecting the same file spans or stale
summaries.

## Boundary

tsift owns context that is independent of a specific agent-doc turn:

- source chunk identity, content hashes, token counts, and source handles
- summary and context-pack generation keyed by input fingerprints
- exact/source-overlap dedupe across packs
- memgraphrag-backed semantic retrieval, memory-event ranking, and
  semantic-near-duplicate signals for large corpora

tsift does **not** own whether a pack has already been shown to a particular
agent session. That belongs to agent-doc because it knows document id, cycle id,
harness, prompt boundary, and restart/compact behavior.

Use the existing memgraphrag direction as the semantic layer. `tsift-memory`
owns durable memory rows; `tsift-memgraphrag` owns lexical+temporal ranking,
memory graph projection, semantic seeded neighborhoods, and graph-db related
retrieval. The dynamic context cache should persist pack/chunk membership around
those retrieval results, not build a second semantic retrieval stack.

## Storage

Add `.tsift/context.db` or extend an existing tsift SQLite store with a distinct
context schema:

- `context_chunks(id, source_uri, source_kind, range_start, range_end, content_hash, token_count, summary, indexed_at, generator_version)`
- `context_packs(id, query, budget, input_fingerprint, rendered_pack, token_count, generated_at, generator_version)`
- `context_pack_chunks(pack_id, chunk_id, rank, role, included_text_hash)`
- `context_pack_aliases(pack_id, stable_handle, source_uri, range_start, range_end)`

The `input_fingerprint` must include query text, budget, source chunk hashes,
index schema version, generator version, and relevant config. Policy stays in
committed files; SQLite stores derived context only.

## Phases

### Phase 1 - `#tsctxschema`: schema and chunk identity

Add the SQLite schema and a library API that can upsert a chunk by
`source_uri + range + content_hash`. Reuse existing index metadata where possible
instead of adding a second file walker. Return stable chunk ids and expansion
handles from source-read/search/context-pack envelopes.

Tests:
- unchanged file/range reuses the same chunk id
- changed content produces a new chunk id
- chunk rows include token count and source handle

### Phase 2 - `#tsctxpackcache`: pack cache by fingerprint

Cache rendered context packs by input fingerprint. Before rendering, resolve the
candidate chunk set, compute the fingerprint, and reuse a valid pack when all
input hashes still match. Store pack->chunk membership so consumers can see what
was represented.

Tests:
- identical query/budget/source hashes hits the cached pack
- changing one source hash invalidates the pack
- changing generator version invalidates the pack

### Phase 3 - `#tsctxdedupe`: exact and overlap dedupe

Teach pack assembly to suppress duplicate context by chunk id, content hash, and
overlapping source ranges. Render a short handle reference when a candidate is
already represented by a higher-ranked chunk in the same pack.

Tests:
- duplicate content hash appears once
- overlapping line ranges collapse to the wider or higher-ranked span
- non-overlapping ranges from the same file can both appear when budget allows

### Phase 4 - `#tsctxmemrag`: memgraphrag-ranked context candidates

Thread memgraphrag into context-pack candidate selection:

- use memory-event ranking for session/task history candidates
- use semantic seeded neighborhoods for concept/entity expansion
- use graph-db related retrieval for source, memory, and semantic rows
- persist the retrieval inputs and ranked node/event ids in pack metadata

This phase should reuse `tsift-memgraphrag` APIs and existing performance/eval
fixtures (`memgraphrag-performance-history.json`, `memory-retrieval-eval.json`)
instead of adding a separate semantic scorer.

Tests:
- memory-event candidates are ranked by relevance plus recency/decay
- semantic seeded neighborhoods contribute bounded candidates
- cache fingerprint changes when the ranked memgraphrag input set changes

### Phase 5 - `#tsctxsemanticdedupe`: memgraphrag near-duplicate hints

Add conservative near-duplicate hints using memgraphrag/semantic graph signals
where available. Mark candidates as near-duplicates without silently dropping
them unless confidence is high. Keep exact/hash/range dedupe as the correctness
path.

Tests:
- exact duplicate still handled without semantic machinery
- high-confidence near duplicate emits a handle reference
- low-confidence similarity keeps both chunks

### Phase 6 - `#tsctxapi`: CLI/library surface

Expose stable commands:

```bash
tsift --envelope context-pack <path-or-query> --budget normal
tsift context cache status
tsift context cache gc
```

Envelope output should include `pack_id`, `chunk_ids`, source handles, token
counts, freshness inputs, and expansion commands.

## Acceptance

- Context packs are reproducible from committed inputs plus generator version.
- Repeated requests do not duplicate unchanged chunks inside a pack.
- Consumers can tell exactly which source chunks a pack represents.
- Semantic and memory context comes through tsift-memgraphrag/graph-db related
  retrieval, not a parallel one-off retriever.
- `make check` passes in `src/tsift`; add fixture coverage for cache hit,
  invalidation, and dedupe.
