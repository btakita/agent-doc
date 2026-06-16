---
agent_doc_session: tsift-v0.1
agent: codex
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

❯ *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc65443c45f7b49f5e6ace54e3f6bafe4077ade57924422b5eb3baf4dbd23c-20260615-195250.md`*

Compacted content:
❯ - Archived 2 response topic(s): #bugs-observed; #advance-review
❯ - Prior summary/context: prior compacted content: Archived 2 response topic(s): #bugs-observed + improve prompt caching; do [#avbq]
- **[#1td4]** crates.io publish job migrated to OIDC trusted publishing. **Code complete and on origin/main** (fe4144b -> f29f7a2; `rust-lang/crates-io-auth-action@v1`, `id-token: write`, no `CARGO_REGISTRY_TOKEN`). Its actionable continuation **already exists in the backlog as [#vmax]** — register the Trusted Publisher for all 23 crates, set `TSIFT_ENABLE_CRATES_PUBLISH=true`, re-tag `v0.1.70` to verify OIDC publish. Those steps require a crates.io account + browser 2FA, which an agent cannot perform, so #vmax is correctly `[operator-verify]` and #1td4 stays `[/]` gated until the operator completes registration.

### Re: #advance-review — opus-4-8

> **Queue prompt:**
>
> #advance-review


Went through `agent:review`. There is exactly one item, and it is already advanced to the agent ceiling — no new backlog item is warranted:

- **[#1td4]** crates.io publish job migrated to OIDC trusted publishing. **Code complete and on origin/main** (fe4144b -> f29f7a2; `rust-lang/crates-io-auth-action@v1`, `id-token: write`, no `CARGO_REGISTRY_TOKEN`). Its actionable continuation **already exists in the backlog as [#vmax]** — register the Trusted Publisher for all 23 crates, set `TSIFT_ENABLE_CRATES_PUBLISH=true`, re-tag `v0.1.70` to verify OIDC publish. Those steps require a crates.io account + browser 2FA, which an agent cannot perform, so #vmax is correctly `[operator-verify]` and #1td4 stays `[/]` gated until the operator completes registration.
- acceptance should be gated by extraction precision/recall fixtures plus the existing MemGraphRAG latency gate; no default-on path until it improves retrieval quality without blowing the budget
- low-confidence or unanchored edges should enter as tentative authored/semantic nodes, not hard graph facts
- model output must be schema-validated JSON with provenance, model/version, confidence, source handles, and deterministic fallback
- deterministic code AST extraction remains authoritative for symbols, spans, call edges, and ownership
- run offline or behind a feature flag during summarize/import/project refresh, where latency and correctness can be measured
- expand query seeds into typed semantic concepts before `graph-db related`
- label limitations, hypotheses, failure modes, benchmark claims, table/figure semantics, and citation-style relationships
- extract `Finding` / `Decision` / `Note` candidates from session/doc text with anchors and confidence
- acceptance should be gated by extraction precision/recall fixtures plus the existing MemGraphRAG latency gate; no default-on path until it improves retrieval quality without blowing the budget
- low-confidence or unanchored edges should enter as tentative authored/semantic nodes, not hard graph facts
- model output must be schema-validated JSON with provenance, model/version, confidence, source handles, and deterministic fallback
- deterministic code AST extraction remains authoritative for symbols, spans, call edges, and ownership
- run offline or behind a feature flag during summarize/import/project refresh, where latency and correctness can be measured
- expand query seeds into typed semantic concepts before `graph-db related`
- label limitations, hypotheses, failure modes, benchmark claims, table/figure semantics, and citation-style relationships
- extract `Finding` / `Decision` / `Note` candidates from session/doc text with anchors and confidence

**Net:** nothing new to add. The review item is already paired with its actionable backlog item; the only thing left is operator-gated crates.io registration — adding another item would just duplicate #vmax. #1td4 stays gated; #vmax stays the operator handle.

This empties the `#advance-review` drain; #vmax is `[operator-verify]` so go-mode defers it (queue has no remaining agent-drainable head).

### Re: small model vs MemGraphRAG — gpt-5

> **Queue prompt:**
>
> :pushpin: Should we use a small model such as a 4B model instead of a MemGraphRag for some cases? See https://www.youtube.com/watch?v=QuTcXMgPanw Use YT transcript or corky gpu.


Short answer: yes, but not as a replacement for MemGraphRAG.

The video transcript argues for separation of concerns, not deleting the graph layer. Around `00:31-01:31`, it says ordinary GraphRAG can build the wrong graph for scientific or long causal reasoning. Around `29:22-29:38`, it frames the engineering win as avoiding a giant LLM for every task because a 4B model can do the extraction/structuring work. Around `31:43-32:33`, that small model is specifically an RL-trained extractor over papers, figures, tables, equations, citations, entities, dependencies, relations, abstractions, and evidence. Then `32:37-33:10` still keeps graph retrieval/traversal and a stronger research/synthesis workflow in the loop.

For tsift, that maps cleanly to: keep MemGraphRAG as the substrate, and consider a small model as an optional ingest/refresh sidecar. The current tsift direction already has decay-weighted memory retrieval, memory-event projection into the shared `GraphStore`, `graph-db related` semantic seeding, ranked neighborhoods, and performance gates. A 4B model should feed that graph with richer typed nodes/edges when deterministic AST/markdown extraction is too shallow; it should not own traversal, ranking, provenance, or final synthesis.

Good target cases:

- extract `Finding` / `Decision` / `Note` candidates from session/doc text with anchors and confidence
- label limitations, hypotheses, failure modes, benchmark claims, table/figure semantics, and citation-style relationships
- expand query seeds into typed semantic concepts before `graph-db related`
- run offline or behind a feature flag during summarize/import/project refresh, where latency and correctness can be measured

Hard guardrails:

- deterministic code AST extraction remains authoritative for symbols, spans, call edges, and ownership
- model output must be schema-validated JSON with provenance, model/version, confidence, source handles, and deterministic fallback
- low-confidence or unanchored edges should enter as tentative authored/semantic nodes, not hard graph facts
- acceptance should be gated by extraction precision/recall fixtures plus the existing MemGraphRAG latency gate; no default-on path until it improves retrieval quality without blowing the budget

So the decision I would record is: use a small local model for structured semantic extraction in some cases, especially scientific/session/doc knowledge enrichment, but do not use it instead of MemGraphRAG. It is an upstream graph-enrichment worker, not the retrieval architecture.

I used the YouTube captions path; no Corky GPU run was needed because captions were available. No code changed this cycle. Verification was document/research only: `tsift status`, `tsift index --workspace .`, `agent-doc preflight`, `agent-doc plan/read`, caption-only `watch.py`, targeted `tsift source-read/search`, and `git status`. Preflight also reported a stale `agent-doc` install warning: rebuild in `src/agent-doc` with `make install` when convenient so live sessions use the current cdylib.

### Re: model support matrix — gpt-5

> **Queue prompt:**
>
> What models will be good to support? I have a 5090. Others may want smaller models.


As of 2026-06-16, I would support model roles first and model names second. tsift should not have one blessed local model; it should expose profiles for `extractor.fast`, `extractor.quality`, `code.quality`, `reasoner.local-high`, `embedding`, and `reranker`.

Your 5090 is a strong high-local target. NVIDIA lists the RTX 5090 with 32 GB of GDDR7 memory, so 20-32B quantized models are realistic for offline graph enrichment, schema extraction, and code-context labeling, assuming context/KV/batch settings are kept sane. But the smallest useful tier still matters because tsift's model sidecar should enrich MemGraphRAG, not become a mandatory frontier-model dependency.

Recommended support matrix:

- **Tiny / CPU / 8 GB class:** Qwen3 0.6B/1.7B/4B, Phi-4-mini 3.8B, Gemma 4 E2B/E4B. Use for cheap tagging, JSON extraction, and smoke tests; never treat output as authoritative graph fact.
- **Small / 8-16 GB class:** Qwen3-8B, Qwen2.5-Coder 7B, Gemma 4 12B, and gpt-oss-20b where the runtime supports its native quantization path. This is the portable default tier for users who want local enrichment without a 5090.
- **Balanced / 16-24 GB class:** Qwen3-14B, Qwen2.5-Coder 14B, Devstral Small/Small 2 24B, and Qwen3-Coder-30B-A3B/Next where supported. This is the best target for higher-quality extraction and code-aware labeling on normal enthusiast hardware.
- **5090 / 32 GB high-local:** Qwen3-32B, Qwen2.5-Coder-32B, Devstral 24B, gpt-oss-20b, and Qwen3-Coder-30B-A3B/Next. This should be tsift's local quality tier for semantic graph enrichment and harder code/document classification.
- **Server/cloud only, not consumer baseline:** gpt-oss-120b, Qwen3-Coder-480B-A35B, and Llama 4 Scout/Maverick. Support them through an OpenAI-compatible endpoint or vLLM/SGLang-style deployment, but do not make them a default expectation.

For retrieval-specific models, I would make embeddings/rerankers first-class instead of overloading the chat model:

- **Embedding default:** Qwen3-Embedding-0.6B or BGE-M3. BGE-M3 is mature and useful for hybrid retrieval because it supports dense, sparse, and multi-vector modes, 100+ languages, and up to 8192 tokens.
- **Embedding quality:** Qwen3-Embedding-4B/8B; optionally Jina Embeddings v4 when visually rich docs, tables, diagrams, or code/doc mixed retrieval become important.
- **Reranker default:** Qwen3-Reranker-0.6B, with 4B/8B as quality upgrades.

The implementation shape should be backend-neutral: OpenAI-compatible HTTP first, with Ollama and llama.cpp as common local adapters. Ollama documents partial OpenAI API compatibility, and llama.cpp has a local HTTP server plus JSON-schema/grammar-constrained output support. That matters more than exact model names because tsift needs schema-valid, provenance-bearing extraction records, not free-form prose.

Default picks I would encode initially:

- `extractor.fast`: Qwen3-4B or Phi-4-mini
- `extractor.quality`: Qwen3-14B
- `code.quality`: Qwen2.5-Coder-14B, with Qwen2.5-Coder-32B / Qwen3-Coder / Devstral for bigger machines
- `reasoner.local-high`: Qwen3-32B or gpt-oss-20b on the 5090
- `embedding.default`: Qwen3-Embedding-0.6B or BGE-M3
- `reranker.default`: Qwen3-Reranker-0.6B

Sources checked: NVIDIA RTX 5090 specs, Qwen3 model family, Qwen2.5-Coder, Qwen3-Coder, Mistral Devstral, OpenAI gpt-oss, Google Gemma, Microsoft Phi, Qwen3 Embedding/Reranker, BGE-M3, Ollama OpenAI compatibility, and llama.cpp grammar/server docs.

No code changed this cycle. Verification was document/research only: `tsift status`, `agent-doc preflight`, `agent-doc plan/read`, current-source web checks for model families, and `git status`. `tsift status` still reports one missing workspace scope, so I did not rely on tsift search/graph evidence for this answer. The worktree had unrelated existing changes; I left them untouched.

Source links: [NVIDIA RTX 5090](https://www.nvidia.com/en-us/geforce/graphics-cards/50-series/rtx-5090/), [Qwen3](https://qwenlm.github.io/blog/qwen3/), [Qwen2.5-Coder](https://qwenlm.github.io/blog/qwen2.5-coder-family/), [Qwen3-Coder](https://github.com/QwenLM/Qwen3-Coder), [Devstral](https://mistral.ai/news/devstral-2-vibe-cli/), [gpt-oss](https://openai.com/index/introducing-gpt-oss/), [Gemma](https://ai.google.dev/gemma/docs/core), [Phi](https://azure.microsoft.com/en-us/products/phi), [Qwen3 Embedding/Reranker](https://github.com/QwenLM/Qwen3-Embedding), [BGE-M3](https://bge-model.com/bge/bge_m3.html), [Ollama OpenAI compatibility](https://docs.ollama.com/api/openai-compatibility), [llama.cpp grammars](https://github.com/ggml-org/llama.cpp/blob/master/grammars/README.md).
<!-- agent:boundary:e08ab740 -->
<!-- /agent:exchange -->

## Queue

<!-- agent:queue preset="#spec-test-build-install-commit-push" go -->
<!-- /agent:queue -->
<!--
https://github.com/chopratejas/headroom
-->

## Backlog

<!-- agent:backlog priority queue -->
- [ ] [#vmax] [operator-verify] Enable crates.io OIDC publishing for tsift (advances review #1td4; code already shipped in fe4144b/f29f7a2, release.yml crates job uses rust-lang/crates-io-auth-action@v1). Operator-gated steps (need crates.io account + browser 2FA — agent cannot perform): (1) On crates.io register a Trusted Publisher for EACH of the 23 tsift crates — GitHub repo btakita/tsift, workflow release.yml, no environment: tsift-core, tsift-md-ast, tsift-graph, tsift-sqlite, tsift-algorithms, tsift-resolution, tsift-cache, tsift-tokensave, tsift-libsql, tsift-index, tsift-summarize, tsift-quality, tsift-agent-doc, tsift-digest, tsift-search, tsift-status, tsift-session, tsift-memory, tsift-memgraphrag, tsift-surrealdb, tsift-cli, tsift, tsift-sim-world. (2) Enable the gate var: gh variable set TSIFT_ENABLE_CRATES_PUBLISH --body true. (3) Force-move the v0.1.70 tag to HEAD and push to re-trigger; confirm the publish-crates.io job authenticates via OIDC (no CARGO_REGISTRY_TOKEN) and publishes.
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
