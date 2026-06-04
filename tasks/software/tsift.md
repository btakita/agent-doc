---
agent_doc_session: tsift-v0.1
resume: 019e85d7-f9ee-79f3-b597-4ed352495bbd
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
queue_active: true
---

## Status

<!-- agent:status patch=replace -->
<!-- /agent:status -->

## Spec

See [src/tsift/SPEC.md](../../src/tsift/SPEC.md) for the AST-Aware Code RAG spec.

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc65443c45f7b49f5e6ace54e3f6bafe4077ade57924422b5eb3baf4dbd23c-20260604-174002.md`*

Compacted content:
- Archived 1 response topic(s): #frontier-unique-temp
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc6544...

### Re: #temp-table-audit — glm-5.1

Audited all temp table usage across `tsift-sqlite` and `tsift-libsql`.

#### Findings

| # | Table | Method | Crate | Transaction | Unique Name | Collision Risk |
|---|-------|--------|-------|-------------|-------------|----------------|
| 1 | `_tsift_frontier_{call_id}` | `shortest_path_with_max_hops` | tsift-sqlite | No | **Yes** (AtomicU64) | None — fixed in `334ca03` |
| 2 | `_edges_between_ids` | `edges_between_nodes` | tsift-sqlite | `unchecked_transaction` | No | **Safe** — see invariants below |
| 3 | `next_graph_nodes`, `next_graph_edges`, `next_graph_node_properties`, `next_graph_edge_properties`, `next_graph_changed_nodes`, `next_graph_changed_edges` | `open_or_refresh` | tsift-sqlite | `transaction` | No | **Safe** — see invariants below |
| 4 | `_edges_between_ids` | `edges_between_nodes` | tsift-libsql | No (async `block_on`) | No | **Safe** — see invariants below |

#### Safety Invariants

**Why concurrent collision is impossible under current architecture:**

1. `rusqlite::Connection` is `Send` but **not `Sync`** — verified in `inner_connection.rs` (`unsafe impl Send for InnerConnection {}`, no `Sync` impl).
2. `SqliteGraphStore` (`conn: Connection`) inherits `Send` but not `Sync`. Multi-thread sharing requires `Mutex<SqliteGraphStore>`, which serializes all method calls.
3. All temp-table methods are synchronous rusqlite calls with no `await` points — they cannot interleave within a single thread even under cooperative async scheduling.
4. `edges_between_nodes` (tsift-sqlite, line 2243) uses `unchecked_transaction()` as an additional isolation layer.
5. `open_or_refresh` projection staging (line 785) uses `self.conn.transaction()` with explicit `DELETE` before use.

**Summary**: Temp table name collisions require concurrent execution on the same `Connection`. Rust's type system (`Connection` not `Sync`) plus synchronous API semantics make this impossible without explicitly unsafe code. The fixed-name tables in #2–#4 are safe under the single-active-call-per-connection invariant.
<!-- agent:boundary:f846d2b1 -->
<!-- /agent:exchange -->
###
<!--
What are #next-steps to promote SurrealDB to SQLite-equivalent substrate status?
-->

## Queue

<!-- agent:queue preset="#spec-test-build-install-commit-push" auto -->
- ~do [#frontier-unique-temp]~
- do [#temp-table-audit]
- do [#sqlite-doc-sync-safety]
- do [#sqlite-reentrant-guard]
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog queue -->
- [ ] [#sqlite-doc-sync-safety] Add doc comments on SqliteGraphStore/SqliteReadOnlyConnection stating single-active-call-per-connection invariant for temp-table methods (medium)
- [ ] [#sqlite-reentrant-guard] [recommended] Add runtime Cell<bool> guard that panics on re-entrant temp-table calls on same connection (low)
<!-- /agent:backlog -->

## Review

<!-- agent:review -->
- [/] [#astnav-npm] Configure NPM_TOKEN + TSIFT_ENABLE_NPM_PUBLISH secrets, publish opencode-tsift to npm (medium)
- [/] [#surreal-batch-bfs] #surreal-batch-bfs-gated: Batch frontier expansion - gated (low)
- [/] [#surreal-fixture] Record SurrealDB samples in graph-db-performance-history.json fixture (medium)
- [/] [#surrealbulk-close] Close #surrealbulk review item: re-run benchmarks, update SPEC.md benchmark table, mark done
- [/] [#rankdefault] Promote ranked_neighborhood into default graph lookup/context-pack/evidence output only after quality gates pass; compare ranked vs unranked output on real backlog/code queries for useful-hit density, duplicate-name precision, handle coverage, latency, and emitted tokens.
- [/] [#surrealbulk] Batch SurrealDB projection load: replace per-row upsert loops with bulk/transactional row load plus one-pass secondary-index rebuild; measure refresh/load separately.
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
- [x] [#lazyperf32-tsift] Cross-posted #lazyperf32 to lazily-rs backlog (lazily-rs project to implement)
- [x] [#lazyperf35-tsift] Cross-posted #lazyperf35 to lazily-rs backlog (lazily-rs project to implement)
- [x] [#lazyperf34-tsift] Cross-posted #lazyperf34 to lazily-rs backlog (lazily-rs project to implement)
- [x] [#lazily-tsift-bench] Cross-posted tsift-pattern benchmarks recommendation to lazily-rs backlog
- [x] [#surreal-rt-pool-warm] Accept external tokio Runtime in open_or_refresh so backend-eval can reuse one Runtime across candidate stores
<!-- completed work archived in tasks/software/tsift.done.md -->
<!-- /agent:done -->
