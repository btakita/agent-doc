---
agent_doc_session: tsift-v0.1
agent: opencode
claude_model: opus
agent_doc_format: template
agent_doc_write: crdt
claude_args: --dangerously-skip-permissions
codex_args: -s danger-full-access
prompt_presets:
  '#agent-doc-bug': Please create a plan for agent-doc to fix this issue. Add to the backlog of agent-loop/tasks/agent-doc/agent-doc-bugs2.md
  '#code-review': Please review tsift codebase. Inspect the architecture and look for edge cases. Isolate concurrency issues and other workflow bugs. Let's find root causes to fix. '#follow-up-backlog'
  '#mermaid': Include explanatory mermaid diagram(s).
  '#next-steps': Any follow-up items to place in the backlog? Please ensure the backlog is in priority order. Print the item id of the top of the backlog.
  '#spec-test-build-install-commit-push': update spec + tests. build + install for local testing. commit + push
  '#spec-test-commit-push': update spec + tests. commit + push
queue_active: false
---

## Status

<!-- agent:status patch=replace -->
<!-- /agent:status -->

## Spec

See [src/tsift/SPEC.md](../../src/tsift/SPEC.md) for the AST-Aware Code RAG spec.

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc65443c45f7b49f5e6ace54e3f6bafe4077ade57924422b5eb3baf4dbd23c-20260601-032539.md`*

Compacted content:
- Archived 6 response topic(s): #terse-health-early-exit; #libsql-edges-between-pushdown; #compact-community-serialization; 3 more
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc6544...
❯ Would tsift benefit from lazily-rs for caching and create lazy-load graphs?
test content


### Re: lazily-rs for tsift caching and lazy-load graphs — glm-5.1

Yes, tsift would benefit from lazily-rs in specific areas, though the fit is narrower than the agent-doc integration (which already has a plan at `tasks/agent-doc/plans/lazily-rs-integration.md`).

#### Where lazily-rs helps tsift

**1. Graph neighborhood expansion** — `ranked_neighborhood()` and `neighborhood()` currently BFS-eagerly to depth N, loading all nodes/edges up front. A lazily-rs Slot per depth layer would defer each hop until the consumer requests it, cutting I/O for shallow queries on large graphs.

**2. Call-graph edge resolution** — `resolve_edges()` resolves all call sites eagerly per file. Wrapping per-symbol resolution in Slots keyed by `(file, symbol, content_hash)` with Cell-backed file-mtime invalidation would avoid re-resolving unchanged symbols across `tsift graph <symbol> --callees` calls.

**3. Thread-local inspection cache replacement** — `InspectScopeGuard` is a hand-rolled dedup cache for `IndexDb::inspect_read_only`. lazily-rs's built-in dependency tracking + auto-invalidation could replace it with less boilerplate.

**4. Summary cache enrichment** — Summary rows could be lazily loaded as Slots keyed by `(file, content_hash)`, computing on first access and auto-invalidating when the file changes (Cell tracks content hash). This replaces the "check if cached, else compute" pattern scattered across summary lookups.

#### Where lazily-rs is NOT a good fit

- **SQLite graph store** (`SqliteGraphStore`) — tsift already has mature SQLite-backed CRUD with WAL concurrency. lazily-rs is single-threaded (`RefCell`-based), so it cannot replace the storage layer.
- **Community detection** (Louvain) — requires full edge-set access; lazy evaluation doesn't help the algorithm itself (though Slot-wrapping the *result* for incremental invalidation on edge changes would).
- **Search token index** — inverted index construction is batch-oriented, not demand-driven.

#### Concrete integration path

```
Cell: content_hash(file)                    — invalidated on file change
 |--
 |-- Slot: symbol_edges(file, content_hash)  — resolve_edges, cached per-file
 |   |--
 |   +-- Slot: symbol_callees(symbol, symbol_edges) -- per-symbol callees
 |
 |-- Slot: neighborhood(depth, center, graph_store) -- lazy BFS layer
 |   |--
 |   +-- Slot: ranked_neighborhood(center, scoring) -- ranked expansion
 |
 +-- Slot: summary(file, content_hash)       -- LLM summary, computed once
```

This is lighter scope than the agent-doc integration (which wires config/frontmatter/SSH resolution). Worth a separate plan if you want to proceed.
What are #next-steps to integrate lazily-rs into tsift?


### Re: #next-steps to integrate lazily-rs into tsift — glm-5.1

Backlog items in priority order:

1. **`#lazily-tsift-plan`** — Create a tsift-specific integration plan. The existing `tasks/agent-doc/plans/lazily-rs-integration.md` covers agent-doc only; tsift needs its own plan covering Context lifetime (single CLI invocation vs. long-lived watch/index daemon), dependency graph layout, and integration points with existing SQLite caches.

2. **`#lazily-inspect-scope`** — Replace `InspectScopeGuard` (`packages/tsift-index/src/index.rs:78`) with lazily-rs reactive cache. This is the lowest-risk entry point: the guard is already a thread-local dedup cache for `IndexDb::inspect_read_only`, so swapping in a lazily-rs `Context` with a Slot per `(db_path, root, prune)` key is a drop-in change with existing test coverage at `lib.rs:31741`.

3. **`#lazily-resolve-edges`** — Wrap `resolve_edges()` (`packages/tsift-graph/src/lib.rs:415`) in lazily-rs Slots. Each file's edge resolution becomes a Slot keyed by `(file, content_hash)` with a Cell tracking file mtime. This avoids re-resolving unchanged call sites across repeated `tsift graph <symbol>` invocations.

4. **`#lazily-neighborhood`** — Convert `ranked_neighborhood()` / `neighborhood()` BFS from eager depth-N expansion to lazy Slots per depth layer. Each layer depends on the previous layer + the `GraphStore`. Consumer only pays I/O for layers actually traversed.

5. **`#lazily-summary`** — Summary cache enrichment: Slots keyed by `(file, content_hash)` that compute LLM summaries on first access and auto-invalidate via Cell-backed content hash tracking. Replaces the scattered "check if cached, else compute" pattern.

**Top of backlog: `#lazily-tsift-plan`**


### Re: status --json auto-fix stale index — glm-5.1

Implemented and pushed (`f454216`).

**What changed:**
- `packages/tsift-cli/src/commands/infra.rs` — `cmd_status` now auto-applies index fixes when `--json` detects a stale or missing index (no WAL lock recovery). This means agents calling `tsift status --json` always get a fresh report without needing `--fix`.
- WAL lock recovery is respected: if the index is stale because a concurrent writer holds a WAL lock (`recovery: Some(...)`), auto-fix is skipped to preserve snapshot fallback reporting.
- `SPEC.md` / `CLAUDE.md` — documented `--json` auto-fix behavior.
- New test: `status_json_auto_fixes_stale_index_without_fix_flag` — confirms `--json` alone refreshes a stale index.

**lazily-rs integration deferred** to a follow-up item (`#lazily-tsift-status-cache`). Adding lazily-rs as a dependency requires workspace-level Cargo.toml changes and a dedicated integration plan beyond the scope of this status auto-fix.
recovery sync
`tsift status` should auto-fix stale indexes by default. Deprecate `--fix`. Add `--no-fix` to skip auto-fixing.
<!-- agent:boundary:782ef24b -->
### Re: auto-fix by default, deprecate --fix, add --no-fix — glm-5.1

Implemented.

**What changed:**
- `packages/tsift-cli/src/cli.rs` — Added `--no-fix` flag to skip auto-fixing. `--fix` is hidden and deprecated.
- `packages/tsift-cli/src/commands/infra.rs` — `cmd_status` now auto-fixes stale indexes by default (no `--json` required). The condition changed from `fix || (json_output && needs_auto_fix)` to `auto_fix && needs_auto_fix` where `auto_fix = !no_fix`. Passing `--fix` prints a deprecation warning to stderr.
- `packages/tsift-cli/src/lib.rs` — Updated dispatch to pass both `fix` and `no_fix` flags; updated unit test call signatures.
- `SPEC.md` — Updated status documentation: examples now show `--no-fix` instead of `--fix`; prose reflects auto-fix-by-default behavior.
- `AGENTS.md` / `CLAUDE.md` — Updated Code Navigation instructions to note `tsift status` auto-fixes by default.

**Tests (all passing):**
- `status_auto_fixes_stale_index_by_default` — confirms `tsift status` (no flags) auto-fixes a stale index
- `status_no_fix_skips_auto_fix` — confirms `--no-fix` preserves stale state in JSON output
- `status_deprecated_fix_flag_shows_warning` — confirms `--fix` emits deprecation warning
- `status_json_auto_fixes_stale_index_without_fix_flag` — still passes (behavior unchanged)
- `status_fix_refreshes_stale_index_before_reporting_json` — still passes (deprecated `--fix` still works)
- `status_fix_refreshes_stale_instructions_after_version_bump_in_json` — still passes
<!-- /agent:exchange -->

## Queue

<!-- agent:queue -->
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog -->
- [ ] [#lazily-tsift-status-cache] [feature] Integrate lazily-rs into tsift status check to cache index inspections and avoid redundant SQLite opens across repeated status calls
- [ ] [#lazily-inspect-scope] [feature] Replace InspectScopeGuard with lazily-rs reactive cache in tsift-index
- [ ] [#lazily-resolve-edges] [feature] Wrap resolve_edges in lazily-rs Slots keyed by (file, symbol, content_hash)
- [ ] [#lazily-neighborhood] [feature] Lazy BFS neighborhood expansion via lazily-rs Slots per depth layer
- [ ] [#lazily-summary] [feature] Lazy summary cache with Cell-backed content_hash invalidation
- [ ] [#lazily-tsift-plan] [plan] Create integration plan for lazily-rs in tsift (see tasks/agent-doc/plans/lazily-rs-integration.md as reference)
<!-- /agent:backlog -->

## Review

<!-- agent:review -->
- [/] [#full-e2e-verify] In fresh project: opencode plugin opencode-tsift && opencode then /tsift-status. On success mark #e2e-install done.
- [/] [#npm-publish-verify] After auth configured, tag release and verify publish-opencode-plugin CI job succeeds and opencode-tsift appears on npmjs.org
- [/] [#npm-auth-setup] Configure NPM_TOKEN secret + TSIFT_ENABLE_NPM_PUBLISH=true variable in GitHub repo settings
- [/] [#e2e-install] Verify end-to-end install: fresh opencode project + opencode plugin opencode-tsift + /tsift-status
<!-- /agent:review -->
###
<!--
Can we increase max-hop caps?
Could FalkorDB help with performance?
-->

<!-- agent:icebox -->
<!-- /agent:icebox -->

## Completed / Reaped

<!-- agent:done archive=tasks/software/tsift.done.md -->

<!-- completed work archived in tasks/software/tsift.done.md -->
<!-- /agent:done -->
