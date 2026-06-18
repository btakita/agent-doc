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

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc65443c45f7b49f5e6ace54e3f6bafe4077ade57924422b5eb3baf4dbd23c-20260618-180930.md`*

Compacted content:
- Archived 9 response topic(s): Integrate memgraphrag + semantic neighborhoods + graphdb as local-model context?; do [#kgleasereap] → implemented the foundational [#kgleasewire] instead; do [#kgextractrefresh]; 6 more
- Prior summary/context: prior compacted content: Archived 1 response topic(s): Is tsift kg now in active usage?

### Re: Operating effectiveness of tsift kg — opus-4-8

> **Queue prompt:** :pushpin: Please let me know the operating effectiveness of tsift kg.

Short version: **the machinery is complete and correct; the operational track record is one document.** I assessed the live graphs and Ollama directly rather than reciting the build log.

**Two layers, very different maturity — measured live:**

| Layer | tsift repo `graph.db` | superproject `graph.db` | Verdict |
|---|---|---|---|
| **Deterministic code graph** (AST, no LLM) | 10,412 nodes / 64,450 edges | 418,358 / 910,292 | **Effective, in active use** |
| **LLM-extracted semantic layer** (`tsift kg extract` → Ollama) | **26 `semantic_entity`, 2 `kg_chunk`, 18 `semantic_relation`** | comparable seed only | **Built + verified, barely exercised** |

- **As a code-RAG substrate it works and is genuinely used.** The structural graph (`calls` 349k, `defines` 347k, `has_ast_span`, `contains`) is large and fresh, and it is what every core command (`search`/`explain`/`graph`/`source-read`/digests) actually runs on — proven independent of the KG/model path by the `#5hed` hermetic guard.
- **As an LLM knowledge graph it has been run essentially once.** The only extracted source in the live graph is `specs/local-kg-model.md` (from `#kgoperatorrun`) → 26 entities. The labels are *coherent*, not garbage (`LocalModelLease`, `GpuProbe`, `qwen3-32b-q4`, `RTX 5090`, `VRAM usage baseline`), so extraction quality on that doc is plausible — but n=1.

**Infra is live and healthy:** Ollama up serving `Qwen3-32B-Q4` (the lease profile); no GPU-lease registry present at idle (expected — the reference-counted lease only exists during an active extract). The full command surface ships and is installed: `extract / status / refresh(+--apply) / evidence / unload / smoke`, plus GraphRAG context injection (`--graph-db` / `--no-context`).

**Two honest gaps the live data exposes:**

1. **No measured effectiveness.** Everything this session built — lease lifecycle (`#kgleasewire`/`#kgleasereap`), staleness refresh (`#kgextractrefresh`/`#kgrefreshapply`), GraphRAG context injection to *reduce duplicate entities* (`#kgctxinject`) — is plumbing that is green in CI but has **no before/after metric on a real corpus**. The dedup improvement the context-pack exists to deliver has never been measured against a baseline.
2. **The one real extraction is provenance-blind.** `tsift kg refresh` confirms live: `Sources: 1, Needs refresh: 1 — specs/local-kg-model.md [NoRecordedHash]`, and **0 of 26 entities carry a recorded confidence**. Both predate the hashing/confidence work, so the staleness + confidence-gating machinery cannot yet protect the only data it has. Self-correcting on the next extract.

**Bottom line:** the *mechanism* is effective and feature-complete; *operational adoption and measurement* are near zero beyond a smoke-scale seed. To actually claim operating effectiveness you would run a multi-doc extraction and measure entity-dedup with/without context injection, the confidence distribution, and one real refresh→reconcile cycle — filed as `[#kgeffmeasure]`. I did **not** kick off a corpus extraction this turn (loads the model / holds the GPU — operator-gated); say the word and I will run the measured before/after. `do [#vmax]` stays gated (crates.io OIDC registration needs your account + browser 2FA).
<!-- agent:boundary:f13bb204 -->
<!-- /agent:exchange -->

## Queue

<!-- agent:queue preset="#spec-test-build-install-commit-push" go -->
- ~~:pushpin: Please let me know the operating effectiveness of tsift kg.~~
- do [#vmax]
- :pushpin: do [#kgeffmeasure]
<!-- /agent:queue -->
<!--
https://github.com/chopratejas/headroom
-->

## Backlog

<!-- agent:backlog priority queue -->
- [ ] [#kgeffmeasure] [operator-verify] [recommended] Measure tsift kg extraction effectiveness on a real corpus (operator-gated: loads Qwen3-32B on GPU, mutates graph.db — needs operator go-ahead): run a multi-doc `tsift kg extract --graph-db` pass, then quantify (a) entity-dedup rate with vs without GraphRAG context injection (#kgctxinject before/after), (b) confidence distribution on emitted semantic_entity nodes, (c) a refresh cycle (edit a source, `tsift kg refresh --apply`, confirm stable-id reconciliation not duplication). Current graph: 1 extracted source / 26 entities / 0 with recorded confidence — machinery complete but operationally unexercised.
- [ ] [#vmax] [operator-verify] Enable crates.io OIDC publishing for tsift (advances review #1td4; code already shipped in fe4144b/f29f7a2, release.yml crates job uses rust-lang/crates-io-auth-action@v1). Operator-gated steps (need crates.io account + browser 2FA — agent cannot perform): (1) On crates.io register a Trusted Publisher for EACH of the 23 tsift crates — GitHub repo btakita/tsift, workflow release.yml, no environment: tsift-core, tsift -md-ast, tsift-graph, tsift-sqlite, tsift-algorithms, tsift-resolution, tsift-cache, tsift-tokensave, tsift-libsql, tsift-index, tsift-summarize, tsift-quality, tsift-agent-doc, tsift-digest, tsift-search, tsift-status, tsift-session, tsift-memory, tsift-memgraphrag, tsift-surrealdb, tsift-cli, tsift, tsift-sim-world. (2) Enable the gate var: gh variable set TSIFT_ENABLE_CRATES_PUBLISH --body true. (3) Force-move the v0.1.70 tag to HEAD and push to re-trigger; confirm the publish-crates.io job authenticates via OIDC (no CARGO_REGISTRY_TOKEN) and publishes.
<!-- /agent:backlog -->

## Review

<!-- agent:review -->
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
