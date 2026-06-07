---
agent_doc_session: tsift-v0.1
resume: 019e85d7-f9ee-79f3-b597-4ed352495bbd
agent: claude
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
queue: start
---

## Status

<!-- agent:status patch=replace -->
<!-- /agent:status -->

## Spec

See [src/tsift/SPEC.md](../../src/tsift/SPEC.md) for the AST-Aware Code RAG spec.

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc65443c45f7b49f5e6ace54e3f6bafe4077ade57924422b5eb3baf4dbd23c-20260607-014719.md`*

Compacted content:
- Archived 2 response topic(s): is [#rtsw] done?; Separate SQLite DBs for caches to avoid writer-lock contention? Is it even an issue with WAL?
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc6544...

### Re: msgpack instead of JSON for serialization — opus-4-8

> **Queue prompt:**
>
> Is there a more efficient way to serialize the data such as msgpack, instead of JSON?


Short answer: **mostly no, and a blanket swap would break things.** I traced where tsift actually serializes data, and only one narrow category is a real msgpack candidate.

**Where JSON is used, and whether msgpack helps:**

1. **SQLite graph columns** — `properties_json`, `provenance_json`, `freshness_json` (`packages/tsift-sqlite/src/lib.rs:713-715`). These look like blobs but are **queried by SQLite itself** via `json_each` / `json_extract` (`lib.rs:401,420`). msgpack would break SQL-side JSON querying. ❌ Keep JSON.
2. **`--json` / `--envelope` stdout** — this is the API contract the harnesses (Claude/Codex/OpenCode) parse. ❌ Must stay JSON.
3. **Session transcript parsing** — `tsift-agent-doc/src/session_*.rs` reads Claude/Codex JSONL. External input format. ❌ Can't change.
4. **The index proper** — symbols/edges/routes are already native SQLite rows/columns (`tsift-index/src/index.rs`), not JSON. Nothing to convert. There are **no dense float embedding vectors** stored (the "hybrid" search is BM25 + structural, not a float-vector store), so the one shape where binary encoding wins big isn't present.

**The only genuine candidate** is the opaque disk caches that round-trip through `serde_json::to_vec` / `from_slice` → `fs::write` and are never SQL-queried:
- `packages/tsift-cli/src/community_detection.rs:225-253`
- `packages/tsift-cli/src/conflict_matrix.rs:1731-1745`

Even there I'd hold off on msgpack specifically:
- **Measure first.** `serde_json` is fast; for these shapes (maps/strings, no big numeric arrays) the gain is ~30-50% size and roughly comparable parse time. If these cache blobs are small relative to the SQLite index, it's noise.
- **If serialization profiles hot, prefer `postcard` or `bincode` over msgpack** for internal-only blobs — they're schema-driven (no field-name overhead), so smaller and faster than msgpack. msgpack's edge (self-describing, cross-language) isn't needed for a cache only tsift reads.
- **For size alone, zstd over the existing JSON** often beats a format swap and is orthogonal — you keep JSON's debuggability.

Queued this as a `[recommended]` backlog item (scoped to those two caches, gated on a profile) so it doesn't get applied as a blanket find-and-replace.
<!-- agent:boundary:0421c4de -->
<!-- /agent:exchange -->
###
<!--
What are #next-steps to promote SurrealDB to SQLite-equivalent substrate status?
-->

## Queue

<!-- agent:queue auto preset="#spec-test-build-install-commit-push" go -->
- ~~Is there a more efficient way to serialize the data such as msgpack, instead of JSON?~~
- Please read https://arxiv.org/pdf/2606.00610. Can tsift become a mem graph rag?
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog priority queue -->
- [ ] [#3dhp] [recommended] Cache encoding: only the opaque disk caches (packages/tsift-cli/src/community_detection.rs read/write_community_detection_cache, conflict_matrix.rs disk cache) are msgpack candidates — they use serde_json to_vec/from_slice → fs::write and are never SQL-queried. Do NOT touch the SQLite graph columns (properties_json/provenance_json/freshness_json) — they are queried via json_each/json_extract; nor the --json/--envelope stdout (harness API contract); nor session JSONL parsing (external format). Gate on a profile showing serialization is hot; if so prefer postcard/bincode (schema-driven, smaller/faster than msgpack for internal blobs) or zstd over the existing JSON. Measure cache blob sizes first.
<!-- /agent:backlog -->

## Review

<!-- agent:review -->
- [/] [#7qky] [recommended] Reduce Codex redundant runbook reads. SHIPPED (v0.34.0 unreleased): retain-don't-reread nudge in shared SKILL Runbooks source -> regenerated all harness surfaces; contract-safe. GATED on re-measurement: re-run 3-harness redundant-read analysis after Codex sessions accumulate with nudge live, compare to baseline (Claude 11.4% / Codex 12.6% / OpenCode 5.3%). If Codex rate doesn't drop, build enforcement as a session-scoped content-dedup LEDGER in a shared LEAF crate (depends on nothing in graph; tsift + agent-doc + others depend down onto it -- NOT inside tsift or agent-doc, avoids dependency cycle). Core = register(id,hash)/should_deliver(id,hash)->Full|Stub, exemptions via hash-change + compaction; session-id passed in by harness (agent-doc cycle_state.rs adapter); thin per-consumer integration. Design tsift-runbook + agent-doc seam to call this ledger from day one. NOT a Claude-only PreToolUse Read hook.
- [/] [#trt1] [recommended] Findings graph layer: add authored Finding/Decision/Note nodes to GraphStore, anchored by stable symbol-handle/tagpath edges (not line numbers), with provenance+confidence+freshness fields gated on community_graph_watermark for staleness; expose retrieval via context-pack/search injection and a / graph-menu command; opt-in/passive capture from agent-doc session archives; md/html as on-demand projection, graph store as source of truth. Spec in src/tsift/SPEC.md.
- [/] [#0y9e] I set NPM_TOKEN="$(passage btak/NPM_TOKEN)". Publish to npm.
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

<!-- completed work archived in tasks/software/tsift.done.md -->
<!-- /agent:done -->
