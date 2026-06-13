---
agent_doc_session: tsift-v0.1
agent: claude
claude_model: opus
agent_doc_format: template
agent_doc_write: crdt
claude_args: --dangerously-skip-permissions
codex_args: -s danger-full-access
prompt_presets:
  '#advance-review': Go through the review items. Add actionable backlog items to complete or advance the review items.,
  '#agent-doc-bug': Please create a plan for agent-doc to fix this issue. Add to the backlog of agent-loop/tasks/agent-doc/agent-doc-bugs2.md
  '#bugs-observed': Please add any bugs from observed behavior to the backlog.,
  '#code-review': Please review tsift codebase. Inspect the architecture and look for edge cases. Isolate concurrency issues and other workflow bugs. Let's find root causes to fix. '#follow-up-backlog'
  '#mermaid': Include explanatory mermaid diagram(s).
  '#next-steps': Any follow-up items to place in the backlog? Please ensure the backlog is in priority order. Print the item id of the top of the backlog.
  '#spec-test-build-install-commit-push': update spec + tests. build + install for local testing. commit + push
  '#spec-test-commit-push': update spec + tests. commit + push
queue: stop
---

## Status

<!-- agent:status patch=replace -->

complete
<!-- /agent:status -->

## Spec

See [src/tsift/SPEC.md](../../src/tsift/SPEC.md) for the AST-Aware Code RAG spec.

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc65443c45f7b49f5e6ace54e3f6bafe4077ade57924422b5eb3baf4dbd23c-20260613-083357.md`*

Compacted content:
- Archived 4 response topic(s): do [#tsreview2]; do [#gdblockcover]; do [#pcacheexplattr]; 1 more
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc6544...

### Re: #bugs-observed — capture observed tsift bugs to backlog — opus-4-8

One reproducible tsift bug observed this session, added to the backlog as **#cargoidxcov**.

**#cargoidxcov — partial cargo-index coverage not surfaced by `status`.** While reading `packages/tsift-agent-doc/src/session_cost.rs` with `tsift source-read`, every call warned `index refs unavailable: no index found at .../indexes/cargo/tsift-agent-doc/index.db` and degraded to window-only output (no AST symbol refs / child refs / graph expansions). Investigation: the workspace has 23 members but `.tsift/indexes/cargo/` holds only 9 (`tsift`, `tsift-cli`, `tsift-core`, `tsift-graph`, `tsift-index`, `tsift-memory`, `tsift-quality`, `tsift-sqlite`, `tsift-status`). The other 14 — including `tsift-agent-doc` — have no per-crate cargo index, so `source-read`/`symbol-read` silently lose symbol projection for ~60% of packages. Yet `tsift status` reports the index `fresh (188 files)` and recommends `use: search, explain, graph` with no signal about the gap. (`tsift search` still resolves symbols via the 20.6M main `index.db`, so search isn't affected — only the per-crate symbol-ref path.) The fix cycle can decide between building cargo indexes for all members on refresh vs. having `status` report partial coverage + a targeted reindex hint.

Out of scope for this doc but noted: the **agent-doc IPC ack-timeout / exit-75-on-finalize** behavior I hit this session is an agent-doc bug, not a tsift one — it belongs in `tasks/agent-doc/agent-doc-bugs2.md` (per the `#agent-doc-bug` preset), so I did not add it to tsift's backlog.

Backlog top: **#cargoidxcov**.

### Re: do [#cargoidxcov] — opus-4-8

Fixed at the root cause and shipped. `source-read` symbol refs and `symbol-read` resolved the per-file/per-scope index target but then only checked whether the db already existed — they never built it. So for any workspace member that no prior `graph`/`search`/`explain` query had indexed, `source-read` degraded to window-only output with `index refs unavailable: no index found at …` and `symbol-read` failed closed, even though `tsift status` reported the shared root index fresh. (Only 9 of 23 members had a per-cargo-package index — ~60% silently lost AST symbol projection.)

Both read paths now join the **same freshness chain** `open_index_db` and the read-only graph queries (`graph`/`communities`/`path`/`explain`) already use: resolve the index target, then `ensure_query_index_current` (build/refresh on demand) before opening. They fall back to the warning / fail-closed error only when the build itself can't complete (e.g. a concurrent writer holds the lock with no prior snapshot).

- **`load_source_symbols`** (`source-read`): replaced the bare `resolve_query_db_path` + existence-check with `resolve_query_index_target` → `ensure_query_index_current` → open.
- **`cmd_symbol_read`** (`symbol-read`): same change; it previously `bail!`ed with "no index found".

**Live proof**: with the `tsift-agent-doc` cargo index absent, `tsift source-read packages/tsift-agent-doc/src/session_cost.rs` now returns `ssym-*` symbol refs (`SessionCostPromptCacheMetadata`) with no warning and builds `.tsift/indexes/cargo/tsift-agent-doc/index.db` on demand.

**Test**: `source_read_symbols_build_cargo_package_index_on_demand` — in a never-queried `crates/core-lib` member (root index built, no per-package index), `load_source_symbols` builds the cargo index on first call and returns the `run` symbol ref with zero warnings.

**Spec**: `specs/search-navigation.md` documents `source-read`/`symbol-read` joining the on-demand-build freshness chain.

**Verification**: `make check` clean (clippy + full suite, exit 0). `cargo install --path .` replaced `~/.cargo/bin/tsift`. Committed + pushed (tsift `c6dc25d`, superproject pointer `43ad2b3a2`). CI run `27462065246` was in progress at finalize — recorded pending, closed out on local evidence.

(Closeout note: this response landed via `--force-disk` after the IPC supervisor's stale-snapshot drift blocked the normal finalize; the `#cargoidxcov` backlog item was already reaped and the queue drained during recovery, so no `--done` was needed.) Backlog + queue are empty.
<!-- agent:boundary:4d31e203 -->
<!-- /agent:exchange -->

## Queue

<!-- agent:queue preset="#spec-test-build-install-commit-push" -->
<!-- /agent:queue -->
<!--
https://github.com/chopratejas/headroom
-->

## Backlog

<!-- agent:backlog priority queue -->
<!-- /agent:backlog -->

## Review

<!-- agent:review -->
<!-- /agent:review -->

<!-- agent:icebox -->
- [ ] [#g5kw] What are #next-steps to promote SurrealDB to SQLite-equivalent substrate status?
- [ ] [#4z0w] Can we increase max-hop caps?
- [ ] [#cg52] Could FalkorDB help with performance?
<!-- /agent:icebox -->

## Completed / Reaped

<!-- agent:done archive=tasks/software/tsift.done.md -->

<!-- completed work archived in tasks/software/tsift.done.md -->
<!-- /agent:done -->
