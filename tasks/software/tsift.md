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

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc65443c45f7b49f5e6ace54e3f6bafe4077ade57924422b5eb3baf4dbd23c-20260619-174251.md`*

Compacted content:
- Archived 4 response topic(s): #next-steps to fix tsift bugs; #kgentitycollapse; #kgconfrank; 1 more
- Prior summary/context: prior compacted content: Archived 4 response topic(s): #kgeffmeasure; measured kg extraction effectiveness on a real ...

### Re: #kgsameas — durable same_as edges for canonical-entity duplicates — opus-4-8

> **Queue prompt:**
>
> do [#kgsameas]


Done. `#kgentitycollapse` deduped only the context-pack retrieval surface; graph-level consumers (`tsift graph`/`explain`/`summarize`, the SurrealDB projection) read `graph_nodes` directly and still saw every per-chunk/per-source duplicate. Now `kg extract` writes durable `same_as` links so all consumers collapse to one logical entity.

- **`SqliteGraphStore::link_nodes_by_shared_property`** — stars nodes of a kind sharing a prefixed id-property value to the group's smallest node id, with link edges of a given kind. Idempotent (edges key on `from/to/kind`), no node deleted (provenance preserved).
- **`tsift-kg::link_canonical_entities_sqlite`** wraps it with KG constants (`semantic_entity` / `entity_id` / `kgent-` / `same_as` / provider+contract).
- **`kg extract`** runs the linker after the per-source replace upsert (`#kgrefreshdup`), so every extraction re-links idempotently.
- spec (`local-kg-model.md`) documents the durable links beside the query-time merge — together they give one logical entity to both the retrieval pack and direct graph traversal.

(Correction to the proposal text: `same_as` was *not* a pre-existing edge kind in `tsift-index` — that grep hit was a test name; I introduced `same_as` as a new KG edge kind here.)

**Verification:** `link_nodes_by_shared_property_stars_duplicates_idempotently` (sqlite — representative choice, idempotency, prefix/singleton skips) + `link_canonical_entities_writes_same_as_edges` (kg wrapper); `cargo test -p tsift-kg -p tsift-sqlite` 60 passed, clippy clean, full `make check` exit 0, `tsift 0.1.70` reinstalled. Committed `1a451a1`, pushed; superproject pointer `93ba03706`.

Next queue head: `do [#kgconfgate]` (provenance-aware min_confidence gating). `#kgconfreqverify` stays gated (operator-verify).

### Re: #kgconfgate — provenance-aware min_confidence gating — opus-4-8

> **Queue prompt:**
>
> do [#kgconfgate]


Done — this finishes the `#kgconf` operability arc. `#kgconfrank` made *ranking* provenance-aware, but the `min_confidence` *gate* still filtered by raw confidence, so a derived `default 0.500` survived a `0.4` gate while a real model `0.300` was excluded.

- **`ContextCandidate`** gains `confidence_is_default` (set only when `confidence_source=default` explicitly), distinct from `confidence_is_model`. Untagged/legacy (pre-`#kgconf`) nodes are **neither** — so they keep working as before.
- **`passes_confidence_gate`** — a positive `min_confidence` excludes explicit derived defaults outright (an unknown default isn't a measured score); model-sourced and untagged/legacy nodes gate by raw confidence; `min_confidence==0` admits everything.
- spec (`local-kg-model.md`) documents the provenance-aware gate as the gating counterpart to `#kgconfrank`.

The three-state distinction (model / explicit-default / untagged-legacy) is the key subtlety: only an *explicit* derived default is gate-excluded, so legacy graphs and the existing raw-gate test keep their behavior.

**Verification:** `confidence_gate_is_provenance_aware` (default 0.9 excluded by a 0.4 gate, model 0.5 + legacy 0.7 admitted) + `confidence_gate_admits_defaults_when_threshold_is_zero`; `cargo test -p tsift-kg` 39 passed, clippy clean, full `make check` exit 0, `tsift 0.1.70` reinstalled. Committed `254eab0`, pushed; superproject pointer `2d7ce6158`.

Queue drained. The KG-quality arc is complete: `#kgconf` (record confidence) → `#kgrefreshdup` (no refresh dups) → `#kgentitycollapse` (retrieval merge) → `#kgconfrank` (rank model > default) → `#kgsameas` (durable same_as) → `#kgconfgate` (gate model > default). Only `#kgconfreqverify` remains, gated as operator-verify (live-GPU A/B).
<!-- agent:boundary:013d61b4 -->
<!-- /agent:exchange -->

## Queue

<!-- agent:queue preset="#spec-test-build-install-commit-push" go -->
<!-- /agent:queue -->
<!--
https://github.com/chopratejas/headroom
-->

## Backlog

<!-- agent:backlog priority queue -->
<!-- /agent:backlog -->

## Review

<!-- agent:review -->
- [/] [#kgconfreqverify] [operator-verify] Verify the confidence-required schema change (#kgconf, payload_schema in packages/tsift-kg/src/ollama.rs) does not reduce extraction yield on the live local model (Qwen3-32B-Q4) via an A/B over a real corpus; if required-confidence cuts entities/relations or induces hallucinated scores, relax to prompt-only + the derived-default fallback. Needs GPU + Ollama (operator-gated).
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
