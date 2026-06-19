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

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc65443c45f7b49f5e6ace54e3f6bafe4077ade57924422b5eb3baf4dbd23c-20260619-170438.md`*

Compacted content:
- Archived 4 response topic(s): #kgeffmeasure; measured kg extraction effectiveness on a real corpus; #kgconf; 1 more
- Prior summary/context: prior compacted content: Archived 1 response topic(s): Operating effectiveness of tsift kg

### Re: #next-steps to fix tsift bugs — opus-4-8

> **Queue prompt:**
>
> What are #next-steps to fix tsift bugs?


The two queue items (`#kgconf`, `#kgrefreshdup`) are done; the backlog was empty. From the gaps those fixes exposed, here are the grounded next steps, now queued in priority order:

1. **`#kgconfrank` (high)** — `#kgconf` makes every node carry a `confidence` + `confidence_source` tag, but nothing consumes the tag yet: `node_to_candidate` in `context_pack.rs` reads only the raw `confidence`, so a *derived* `default 0.500` outranks and out-survives a *real* model `0.300` under `min_confidence`/ranking. Weight model-emitted confidence above derived defaults. This directly completes the operability `#kgconf` set up, so it leads.
2. **`#kgentitycollapse` (med)** — `#kgctxinject` dedup is naming-convergence + suppression, **not** node-identity collapse: node id = `stable_id(chunk.id, entity.id)`, so same-label entities from different chunks stay distinct nodes (two dup nodes shared one canonical `entity_id` but had different node ids — finding (c)). Add `same_as` edges keyed on the canonical `entity_id` (or a query-time merge) so retrieval sees one logical entity; preserve per-source provenance by linking rather than deleting.
3. **`#kgconfreqverify` (operator-verify)** — `#kgconf` made `confidence` a *required* schema field to elicit real scores. Verify on the live Qwen3-32B model that this doesn't cut extraction yield or induce hallucinated scores; relax to prompt-only + default fallback if it does. Needs GPU + Ollama, so it's operator-gated.

**Top of backlog: `#kgconfrank`.**

(Heads-up: the exchange is at `session_accretion: warn` — 80 lines / 4 response sections. Say the word and I'll `agent-doc compact tasks/software/tsift.md --commit`.)

### Re: #kgentitycollapse — query-time merge of canonical-entity duplicates — opus-4-8

> **Queue prompt:**
>
> do [#kgentitycollapse]


Done. The extractor reconciles a recurring entity to a canonical `kgent-…` id (stored as the `entity_id` property), but each chunk/source still projects a distinct node id for it, so the graph holds several provenance-bearing nodes for one logical entity — `#kgctxinject` dedup is naming-convergence + suppression, not node-identity collapse. Retrieval surfaced each as a separate entity.

I took the **query-time identity merge** (the backlog's safer option — no graph mutation, preserves all per-source provenance):

- **`context_pack.rs` — `collect_candidates_from_nodes`** now collapses candidates that share a canonical `entity_id` to one representative (highest confidence → group's max degree → smallest node id, fully deterministic). The graph keeps every node; only the *retrieval surface* dedups.
- **`canonical_entity_id()`** treats only `kgent-`-prefixed ids as merge keys, so chunk-local model slugs (`e0`/`e1`) never wrongly merge two genuinely distinct entities.
- The bounded-scan cap now counts **nodes scanned** rather than deduped output, so the never-dump-the-graph guarantee still holds with merging.
- spec (`local-kg-model.md`) documents the merge under the context-retrieval contract.

**Verification:** `collapses_candidates_sharing_canonical_entity_id` (merged rep keeps max degree; distinct canonical id + local-slug node stay separate); `cargo test -p tsift-kg` 34 passed, clippy clean, full `make check` exit 0, `tsift 0.1.70` reinstalled. Committed `0648af5`, pushed; superproject pointer `ee8437d34`.

Next queue head: `do [#kgconfrank]` (weight `confidence_source` so derived defaults don't outrank real model scores). `#kgconfreqverify` stays operator-gated.

### Re: #kgconfrank — model confidence now outranks derived defaults — opus-4-8

> **Queue prompt:**
>
> do [#kgconfrank]


Done. `#kgconf` records a `confidence_source` tag (`model|default`) on every node, but the context-pack ranker still read only raw `confidence` — so a derived `default 0.500` outranked and out-survived a real model `0.300`, defeating the point of recording confidence.

- **`ContextCandidate`** now carries `confidence_is_model` (read from the `confidence_source` property; only an explicit `=model` counts as a real score).
- **`build_context_pack`** inserts a confidence-provenance tier into the deterministic sort, between connectivity and raw confidence: `seed match → degree → provenance tier (model before default) → confidence desc → node-id`. So at equal connectivity a real model `0.300` ranks above an unknown `0.500` default; within a tier raw confidence still decides.
- spec (`local-kg-model.md`) documents the provenance tier in the ranking contract.

**Verification:** `model_confidence_outranks_derived_default` + `within_model_tier_higher_confidence_wins`; `cargo test -p tsift-kg` 36 passed, clippy clean, full `make check` exit 0, `tsift 0.1.70` reinstalled. Committed `1ab19d5`, pushed; superproject pointer `6ecfe25e1`.

Queue drained — the three KG follow-ups from this round (`#kgentitycollapse`, `#kgconfrank`) are done; `#kgconfreqverify` is gated in review as operator-verify (needs live GPU + Ollama A/B). No drainable work remains.
<!-- agent:boundary:0c3aa787 -->
<!-- /agent:exchange -->

## Queue

<!-- agent:queue preset="#spec-test-build-install-commit-push" go -->
- :pushpin: What are #next-steps to fix tsift bugs?
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
