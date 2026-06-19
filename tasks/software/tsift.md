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
queue: start
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

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc65443c45f7b49f5e6ace54e3f6bafe4077ade57924422b5eb3baf4dbd23c-20260619-051804.md`*

Compacted content:
- Archived 1 response topic(s): Operating effectiveness of tsift kg
- Prior summary/context: prior compacted content: Archived 9 response topic(s): Integrate memgraphrag + semantic neighborhoods + graphdb as lo...

### Re: #kgeffmeasure — measured tsift kg extraction effectiveness on a real corpus — opus-4-8

Operator-approved. Ran a live multi-doc `tsift kg extract` A/B (Qwen3-32B-Q4 on GPU) over `specs/local-kg-model.md` + `specs/memgraphrag-direction.md` into **scratch DBs** (`/tmp/kg-eval/*.db`) — the live `.tsift/graph.db` (26 entities) and the tsift working tree are **untouched**. Three questions, three concrete findings:

**(a) Entity-dedup via GraphRAG context injection (#kgctxinject) — works, ~15% fewer nodes, concentrated on later docs.**
- No context (`--no-context`): **71** entities (doc1=46, doc2=25).
- With context injection (default): **60** entities (doc1=51, doc2=**9**).
- The injected known-entity pack hits the *second* document hardest: doc2 new entities **25 → 9 (−64%)**, while exact cross-doc label agreement rose **2 → 7** shared labels. So later docs reconcile toward canonical names and emit far fewer redundant entities. Caveat: dedup is *naming convergence + suppression*, not node-identity collapse — same-label entities from different chunks still get distinct node ids (see finding c).

**(b) Confidence distribution — none. The projection never persists confidence.**
- **0 of 71** freshly-extracted `semantic_entity` nodes carry a real `confidence` property (the only property keys are `chunk_id, contract, description, document_id, entity_id, entity_kind, kg_extraction_model, kg_provider, kg_provider_kind, provider, source_ref`). The "26 with 0 confidence" in the live graph isn't a one-off — the extractor/projection emits no confidence field at all, so the spec's confidence/recency gating has nothing to gate on. → filed **#kgconf**.

**(c) Refresh cycle — staleness detection works; reconciliation is additive and duplicates entities.**
- Watermark staleness works: editing the source flipped `Unchanged → Stale`; `refresh --apply` re-extracted 1, 0 failed.
- But refresh is **purely additive**: re-extracting one edited source went **9 → 18** entities — all 9 prior stable ids retained (good, no id churn) yet **9 new nodes added with no tombstone of the prior projection**, yielding duplicate canonical entities for a single source: `tsift-memgraphrag ×3`, `.tsift/graph.db ×3`, `MemGraphRAG ×2`, `#kgwiring ×2`.
- Root cause: the node primary-key id derives from chunk+description, **not** the canonical `entity_id` — which *is* present as a property and *does* match across passes (two dup nodes shared `entity_id=kgent-72df35b3…` but had different node ids). The reconciliation key exists in the data; the upsert just doesn't use it. Since LLM output is non-deterministic, every refresh accumulates duplicates. → filed **#kgrefreshdup**.

**Net:** the KG machinery runs end-to-end on a real corpus, context-injection dedup is real and measurable, but two operational gaps block it from being production-grade: confidence is never recorded (#kgconf) and refresh duplicates entities (#kgrefreshdup) — both now top of backlog as actionable tsift code work.

Not auto-looping: the only remaining queue head is `do [#vmax]`, which is operator-gated (crates.io Trusted Publisher registration for 23 crates + browser 2FA — agent cannot perform). `queue_continuation_required=false`.

### Re: measured kg extraction effectiveness on a real corpus — opus-4-8

Operator-approved, so I ran the live measurement. A/B extraction (Qwen3-32B-Q4 on GPU) over two related specs — local-kg-model.md and memgraphrag-direction.md — into scratch DBs under /tmp/kg-eval. The live .tsift/graph.db (26 entities) and the tsift working tree were left untouched and verified clean afterward. Three questions, three findings.

**(a) Context-injection dedup works, ~15% fewer nodes, concentrated on later documents.** Extraction without context injection produced 71 entities (doc1=46, doc2=25). With the default known-entity context pack it produced 60 (doc1=51, doc2=9). The pack hits the second document hardest: its new entities fell from 25 to 9, a 64% drop, while exact cross-document label agreement rose from 2 to 7 shared labels. So later documents reconcile toward canonical names and emit far fewer redundant entities. Caveat: the dedup is naming convergence plus suppression, not node-identity collapse — same-named entities from different chunks still land as distinct node ids (see c).

**(b) Confidence is never recorded.** Zero of all 71 freshly-extracted entity nodes carry a real confidence property; the only keys present are chunk_id, contract, description, document_id, entity_id, entity_kind, kg_extraction_model, kg_provider, kg_provider_kind, provider, source_ref. The "26 with 0 confidence" in the live graph is not a one-off — the projection emits no confidence field at all, so the spec's confidence/recency gating has nothing to act on. Filed as a backlog follow-up (kgconf).

**(c) Refresh detects staleness but reconciliation is additive and duplicates entities.** Editing a source correctly flipped its watermark from Unchanged to Stale, and refresh --apply re-extracted it cleanly. But the re-extraction is purely additive: one source went from 9 to 18 entities — all 9 prior stable ids were retained (no id churn), yet 9 new nodes were added with no tombstone of the prior projection, leaving duplicate canonical entities for a single source (tsift-memgraphrag appeared 3 times, the graph-db path 3 times, MemGraphRAG twice). Root cause: the node primary-key id derives from chunk plus description, not the canonical entity_id — which is already present as a property and does match across passes (two duplicate nodes shared one canonical entity_id but had different node ids). The reconciliation key exists in the data; the upsert just does not key on it. Filed as a backlog follow-up (kgrefreshdup).

Net: the machinery runs end-to-end on a real corpus and the context-injection dedup is real and measurable, but two operational gaps block production-grade use — confidence is never persisted, and refresh duplicates entities. Both are now top of the backlog as actionable tsift code work, and the measurement item is done.

### Re: #kgconf — KG projection now always persists per-entity confidence — opus-4-8

> **Queue prompt:**
>
> do [#kgconf]


Root cause confirmed and fixed. The projection layer (`entity_node`/`relation_edge` in `packages/tsift-kg/src/lib.rs`) only wrote a `confidence` property `if entity.confidence.is_some()`. Local-model structured output (Qwen3) drops the optional field, so every freshly-extracted node arrived with `confidence: None` → 0 of 71 nodes carried confidence → `node_to_candidate` in `context_pack.rs` read the missing property as `0.0`, making the `#kgctxinject` `min_confidence` gate and confidence-descending ranking inoperable (every candidate tied at 0.0).

**Fix (3 files, committed `ac423b1`, pushed to origin/main; superproject pointer `bcfae6ce4`):**

- **`lib.rs` — universal guarantee in the projection layer.** `entity_node` and `relation_edge` now *always* persist `confidence` (3-decimal string) plus a `confidence_source` tag: the model value when present (`source=model`), else a neutral derived `DEFAULT_KG_CONFIDENCE = 0.5` (`source=default`). This covers every extractor, not just Ollama, so the gate always has data and can weight model-emitted scores above defaults.
- **`ollama.rs` — better real signal.** Marked `confidence` `required` on both the entity and relation structured-output schemas so the decoder emits a real per-fact score instead of silently dropping the optional field.
- **`specs/local-kg-model.md`** — documents the always-persist guarantee under Graph Provenance.

**Verification:** new test `projection_always_persists_confidence_with_source_tag` (model value tagged `model`, omitted value → default tagged `default`, on both nodes and edges) + extended schema-required assertions. `cargo test -p tsift-kg` 32 passed, `clippy` clean, full `make check` exit 0, `cargo install` (root + cli) → `tsift 0.1.70` live. CI run 27812097553 was pending at push time (recorded, not blocking).

Next queue head: `do [#kgrefreshdup]` (refresh accumulates duplicate canonical entities — key the upsert on canonical `entity_id`).
<!-- agent:boundary:78284515 -->
<!-- /agent:exchange -->

## Queue

<!-- agent:queue preset="#spec-test-build-install-commit-push" go -->
- ~~:pushpin: [#kgeffmeasure]: approved~~
- ~~do [#vmax]~~
- ~~do [#kgconf]~~
- do [#kgrefreshdup]
<!-- /agent:queue -->
<!--
https://github.com/chopratejas/headroom
-->

## Backlog

<!-- agent:backlog priority queue -->
- [ ] [#kgrefreshdup] tsift kg refresh --apply accumulates duplicate canonical entities. Re-extracting one edited source went 9→18 entities (verified 2026-06-19): all 9 prior stable ids retained, but 9 NEW nodes added with NO tombstone of the prior source projection → tsift-memgraphrag x3, .tsift/graph.db x3, MemGraphRAG x2, #kgwiring x2 for a single source after one refresh. Root cause: node primary-key id derives from chunk+description, not the canonical entity_id (already present as a property and matching across passes). Key the node upsert on canonical entity_id, or tombstone the prior source projection before re-insert.
<!-- /agent:backlog -->

## Review

<!-- agent:review -->
- [/] [#vmax] [operator-verify] Enable crates.io OIDC publishing for tsift (advances review #1td4; code already shipped in fe4144b/f29f7a2, release.yml crates job uses rust-lang/crates-io-auth-action@v1). Operator-gated steps (need crates.io account + browser 2FA — agent cannot perform): (1) On crates.io register a Trusted Publisher for EACH of the 23 tsift crates — GitHub repo btakita/tsift, workflow release.yml, no environment: tsift-core, tsift -md-ast, tsift-graph, tsift-sqlite, tsift-algorithms, tsift-resolution, tsift-cache, tsift-tokensave, tsift-libsql, tsift-index, tsift-summarize, tsift-quality, tsift-agent-doc, tsift-digest, tsift-search, tsift-status, tsift-session, tsift-memory, tsift-memgraphrag, tsift-surrealdb, tsift-cli, tsift, tsift-sim-world. (2) Enable the gate var: gh variable set TSIFT_ENABLE_CRATES_PUBLISH --body true. (3) Force-move the v0.1.70 tag to HEAD and push to re-trigger; confirm the publish-crates.io job authenticates via OIDC (no CARGO_REGISTRY_TOKEN) and publishes.
- [/] [#1td4] Release infra: crates.io publish job in .github/workflows/release.yml is MIGRATED to OIDC trusted publishing (commit fe4144b, on origin/main f29f7a2; uses rust-lang/crates-io-auth-action@v1, id-token: write, no CARGO_REGISTRY_TOKEN). Code complete. Remaining is operator-gated and tracked as actionable backlog #vmax: register the Trusted Publisher per crate on crates.io (23 crates), set repo var TSIFT_ENABLE_CRATES_PUBLISH=true, then re-tag v0.1.70 to verify OIDC publish. Stays gated until operator completes crates.io registration.
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
