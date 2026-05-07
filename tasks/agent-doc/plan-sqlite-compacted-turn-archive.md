# Plan — sqlite index for compacted-turn archive lookup

**Pending:** `#sqlarcidx`
**Status:** Draft

## Problem

`agent-doc compact` writes older exchange content into markdown archive files
under `.agent-doc/archives/`, then leaves only a short `### Session Summary`
plus the archive path in the live document. That keeps the active session small,
but it also means archived turns are expensive to query:

- no indexed lookup by document, session id, backlog id, or prompt text
- no ranking that prefers recent/archive-adjacent context over stale history
- no bounded retrieval layer for "find the last related compacted turn"
- no policy for keeping any future retrieval cache from growing without bound

The existing sqlite investigation in
`tasks/agent-doc/plan-sqlite-session-store-concurrency.md` is about
authoritative coordination state. This archive problem is different: it is a
retrieval/indexing problem over immutable compacted history.

## Goal

Define a bounded sqlite-backed archive index that makes compacted turns fast to
find without changing the current source-of-truth model:

- live session document stays authoritative for current state
- markdown archive files remain the durable history artifact
- sqlite acts as a rebuildable retrieval index/cache over those archives

## Non-Goals

- Do not turn sqlite into the new authority for live exchange content.
- Do not require every query path to understand raw SQL.
- Do not delete or rewrite archive markdown files as part of first rollout.
- Do not introduce a graph database unless concrete query evidence shows sqlite
  cannot cover the retrieval workload.

## Design Constraints

- The cache/index must stay project-local under `.agent-doc/`.
- The DB must be safe to rebuild from archive markdown plus current docs.
- Retrieval must account for temporal drift: recent turns should rank above old
  turns by default, and callers must be able to filter by time/document/session.
- Growth must be explicitly bounded; "keep everything forever" is not an
  acceptable default for the sqlite layer.

## Proposed Model

Use a dedicated sqlite file such as `.agent-doc/archive-index.db` as a derived
index over compacted exchange history.

### Archive file remains canonical history

- `agent-doc compact` still writes full markdown archives to
  `.agent-doc/archives/*.md`
- the live document still keeps the compact summary and archive pointer
- the sqlite DB can be dropped and rebuilt without losing history

### sqlite stores searchable metadata and chunks

Candidate tables:

- `archives`
  - archive path
  - document canonical path
  - session id
  - compacted_at timestamp
  - source snapshot hash
  - archive byte size
  - last_indexed_at timestamp
- `archive_turns`
  - archive id
  - turn ordinal
  - speaker (`user` / `assistant`)
  - turn timestamp when recoverable
  - normalized text
  - content hash
- `archive_refs`
  - archive id
  - extracted backlog ids / tags / linked plan paths
  - optional typed references for exact lookup

Use FTS5 over normalized turn text, then rank with a recency bias and exact
match boosts for document path, session id, backlog id, and nearby archive age.

## Bounded Growth Policy

Treat sqlite as a bounded index/cache, not as the only copy of the archives.

Default policy:

- archive markdown files remain on disk until a separate retention policy says
  otherwise
- sqlite rows are eligible for pruning/rebuild because they are derived
- keep a configurable upper bound by:
  - max indexed archives
  - max DB size in bytes
  - max age for cold entries

Preferred eviction strategy:

- use recency plus pinning, not pure LRU
- pin:
  - archives referenced by currently open session summaries
  - archives touched within a recent time window
  - archives referenced by active backlog items or explicit lookups
- evict cold, unpinned rows first when a size cap is exceeded

Reasoning: pure LRU is too blunt because an old archive may still be relevant to
an active bug id or currently open document. A hybrid of recency, pins, and
rebuildability is safer.

## Query Shapes To Support

- "Find the most recent compacted turn about `#id` in this document"
- "Search compacted history for text similar to the current prompt"
- "List archives for this session/doc ordered by recency"
- "Given an archive path in `### Session Summary`, load its indexed metadata"
- "Show whether a hit is stale compared with the current live document"

## Suggested Rollout

### Phase 1: evidence + retrieval contract

- Document concrete retrieval cases where archive lookup would save a turn or
  avoid rereading large markdown files manually.
- Define the ranking contract: recency-first, document-local-first, exact id/tag
  boosts, and stale-hit labeling.

### Phase 2: offline indexer

- Add a command that scans `.agent-doc/archives/*.md` and builds/rebuilds the
  sqlite index.
- Keep this entirely read-only with respect to live session flows.

### Phase 3: incremental indexing on compact

- After `agent-doc compact`, upsert the new archive into sqlite in the same CLI
  run.
- Fail soft if indexing breaks: archive markdown creation must still succeed.

### Phase 4: retrieval surface

- Add a binary surface for archive search/lookup so agents and plugins do not
  reach into sqlite directly.
- Return result metadata including recency, source archive path, and any stale
  warning relative to the live document.

### Phase 5: bounded-prune + rebuild tooling

- Add pruning based on configured size/age caps.
- Add a deterministic rebuild command that drops and recreates the DB from the
  archive markdown corpus.

## Open Questions

1. Should the archive index live in the same sqlite file as any future
   authoritative session store, or stay separate because it is derived/cacheable?
2. How much structure should be extracted from archive text on ingest:
   raw FTS only, or ids/tags/plan-path extraction too?
3. Should archive pruning ever delete markdown history, or only trim sqlite
   rows while leaving archive files intact?
4. What should the default stale-warning heuristic be when a hit predates the
   current live document by several compaction cycles?

## Likely Files

- `src/agent-doc/src/compact.rs`
- `src/agent-doc/src/main.rs`
- `src/agent-doc/src/search.rs` or similar new module
- `src/agent-doc/src/store/` or a dedicated archive-index module
- `tasks/agent-doc/plan-sqlite-session-store-concurrency.md`

## Exit Criteria

1. `agent-doc` has a concrete plan for fast lookup of compacted turns without
   changing the live document authority model.
2. The sqlite layer is explicitly bounded and rebuildable.
3. The design distinguishes durable markdown archives from derived index/cache
   rows.
4. The rollout can start with read-only indexing before any live retrieval
   integration.
