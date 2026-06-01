# Plan: lazily-rs Integration for tsift

**Status:** Implemented roadmap, with follow-on gates documented
**Created:** 2026-06-01
**Session:** `tasks/software/tsift.md`
**Queue item:** `#lazily-tsift-plan`

## Goal

Use `lazily-rs` where tsift repeatedly derives the same local state within one CLI invocation: index inspections, status checks, call-edge resolution, graph-neighborhood expansion, and summary-cache extraction decisions. The integration should reduce duplicated SQLite/file/AST work without replacing tsift's SQLite stores, WAL/snapshot recovery, or explicit write locks.

## Current State

The lazily-rs integration is now present in five tsift surfaces:

- `tsift-status::StatusCheckCache` caches index inspection results during one status command and invalidates after any status-triggered index mutation.
- `tsift-index::InspectScopeGuard` wraps trusted context-pack/session-review inspection pipelines in a lazily-backed per-thread scope.
- `tsift-graph::ResolveEdgesCache` caches per-file call-edge resolution keyed by `(file, content_hash)` with an mtime Cell dependency.
- `tsift-core::GraphStore::{neighborhood,ranked_neighborhood}` builds lazy layer Slots for BFS/ranked graph expansion.
- `tsift-summarize::SummaryCache` caches per-file summary snapshots keyed by normalized file path with a content-hash Cell dependency, and owns the "current rows or extract then replace" branch for `summarize --extract`.

This plan records the intended shape and the gates for keeping the integration safe as tsift grows.

## Context Lifetimes

| Context | Lifetime | Owner | Invalidation |
|---------|----------|-------|--------------|
| Status cycle | One `tsift status` invocation | `StatusCheckCache` | Explicit epoch bump after status applies index/instruction fixes |
| Inspect scope | One guarded call chain on one thread | `InspectScopeGuard` | Explicit epoch bump after in-scope index refresh; scope drops at outer guard |
| Index update | One incremental indexing pass | `ResolveEdgesCache` | Content-hash key split plus mtime Cell dependency |
| Graph query | One `GraphStore` neighborhood call | local `LazyContext` in trait default methods | Context drops after query; each layer depends on prior layer |
| Summary extraction | One `summarize --extract` invocation | `SummaryCache` | Content-hash Cell update per file; explicit invalidation after delete/replace |

The current integration intentionally stays per-command or per-scope. A long-lived daemon/watch context is not part of the current contract; it would need a separate invalidation model tied to file watcher events and SQLite writer notifications.

## Dependency Graphs

### Status / Inspection

```text
Cell: epoch
  |
  +-- Slot: inspect(db_path, root, prune)
        |
        +-- IndexDb::inspect_read_only(...)
```

The same `(db_path, root, prune)` is inspected once while the epoch is unchanged. Status mutations and guarded refreshes bump the epoch, clearing dependent Slots before the next read.

### Call-Edge Resolution

```text
Cell: file_mtime(file, content_hash)
  |
  +-- Slot: resolve_edges(file, content_hash)
        |
        +-- resolve_edges_uncached(symbols, call_sites)
```

The content hash splits semantic versions of a file. The mtime Cell protects against same-hash metadata changes that should still force the Slot dependency graph to observe the file state before reuse.

### Graph Neighborhoods

```text
Slot: layer_0(center)
  |
  +-- Slot: layer_1(frontier_0)
        |
        +-- Slot: layer_2(frontier_1)
              |
              +-- ...
```

Layer Slots stop being created when the frontier is empty or the requested depth is reached. Ranked neighborhoods use the same structure around the scoring queue and prune before expanding low-priority nodes.

### Summary Extraction

```text
Cell: requested_content_hash(file)
  |
  +-- Slot: summary_snapshot(file)
        |
        +-- SummaryDb::get_by_file(file)
        +-- current = any(row.content_hash == requested_content_hash)
```

`SummaryCache::get_or_extract_file` first reads the Slot. If current rows exist, extraction is skipped. If the Slot reports missing or stale rows, the caller computes summaries and `SummaryCache` replaces the file rows, then invalidates the Slot with the new content hash.

## Implementation Phases

### Phase 1: Status Cache

**Status:** Done in `8a87c26` (`feat(status): cache status index inspections`)

- Add `lazily` to `tsift-status`.
- Cache `IndexDb::inspect_read_only` per status cycle.
- Reuse tracked file paths for summary coverage.
- Invalidate after status-triggered index mutations.

### Phase 2: Inspect Scope

**Status:** Done in `22e6768` (`feat(index): lazily cache inspect scopes`)

- Replace the thread-local `HashMap` inspection cache with lazily Slots.
- Preserve unguarded fresh-read behavior.
- Clear failed Slots so transient read failures remain retryable.

### Phase 3: Edge Resolution

**Status:** Done in `e6b0660` (`feat(graph): lazily cache edge resolution`)

- Add `ResolveEdgesCache` in `tsift-graph`.
- Keep the existing pure `resolve_edges()` API.
- Wire incremental indexing through the cache.

### Phase 4: Graph Neighborhoods

**Status:** Done in `bba1c66` (`feat(core): lazily layer graph neighborhoods`)

- Move unranked BFS expansion into lazily layer Slots.
- Move ranked expansion into lazily Slots over the scoring queue.
- Preserve ordering, edge-kind filters, pruning, and diagnostics.

### Phase 5: Summary Cache

**Status:** Done in `20fedad` (`feat(summarize): lazily cache summary extraction`)

- Add `SummaryCache` in `tsift-summarize`.
- Key summary snapshots by normalized file path.
- Track requested live content hash through a Cell.
- Wire `summarize --extract` through `get_or_extract_file`.

## Non-Goals

- Do not replace SQLite stores with lazily-rs. SQLite remains the durable store for indexes, graph projections, summaries, memory, and operator state.
- Do not cache across CLI invocations until tsift has a long-lived daemon/watch owner with explicit invalidation.
- Do not use lazily-rs for full community detection or inverted-index construction; those algorithms need full input sets.
- Do not weaken read-only snapshot fallback or writer-lock behavior.

## Verification Contract

Every lazily-rs integration point needs both unit-level cache behavior coverage and a higher-level command path:

- Cache behavior: hit/miss or reuse-until-invalidation assertions.
- Failure behavior: failed Slot results must not poison future reads when the old code retried.
- Command behavior: the existing command path must still pass under `make check`.
- Lock behavior: read-only snapshot/WAL fallback tests must keep passing for affected packages.
- CI evidence: GitHub CI must pass both `clippy + test` and `live Convex graph acceptance`.

## Future Gates

These are not active backlog items. They become actionable only if tsift adds a long-lived watch daemon or a persistent query worker.

- Define an `ActorContext` lifetime with file watcher invalidation.
- Add cache telemetry to command JSON when it helps operators diagnose repeated work.
- Decide whether cross-command caches should live in SQLite, not lazily-rs, for multi-process correctness.
- Benchmark `SummaryCache` on a large real extraction run before adding more summary-layer caching.

