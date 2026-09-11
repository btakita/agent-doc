---
agent_doc_session: tsift-v0.1
resume:
  claude: 7ec8a9b0-c6b3-46ca-b057-dfbb94db6d6d
  codex: 01a08387-676c-7d10-9667-16c9ddf4765a
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
queue: go
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

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc65443c45f7b49f5e6ace54e3f6bafe4077ade57924422b5eb3baf4dbd23c-20260909-001729.md`*

Compacted content:
- Archived 6 response topic(s): tsift 0.1.96 released; Response; do [#fixcompactexchange]; 3 more
- Prior summary/context: prior compacted content: Archived 1 response topic(s): tsift issue #23 and v0.1.95

### Re: Session resumed — gpt-6

Recorded the updated Codex resume ID. The queue remains stopped; issue #25 is pending.

Workspace verification: `make check` failed in `corky-core` library tests (`make` exit 2; log: `.tsift/artifacts/tart-2ff5828b2d.test.log`). No source changes were made. The latest GitHub Actions run passed; no workflow named `CI` exists in this workspace. The stale agent-doc install warning remains advisory for its owning development cycle.


### Re: Issue #25 fixed and v0.1.97 published — gpt-6

> **Queue prompt:** Fix https://github.com/btakita/tsift/issues/25 + release + publish

Fixed and closed [issue #25](https://github.com/btakita/tsift/issues/25) in commit `4e8af21`, pushed to `main` and tagged `v0.1.97`.

- `large_projection` remains visible as an informational check; an otherwise healthy doctor report now returns `ok` with no repair commands. Genuine warnings and fail-closed checks retain their severity.
- Compaction and retention share one count result. Reports distinguish retained tombstones from compaction count-scan rows; cached counts report zero scanned rows. Regression coverage denies graph-row reads to prove the cached storage path, and the legacy fallback uses SQL primary-key probes instead of collecting all live keys in memory. Other doctor integrity checks still run.
- Ordinary source-watermark drift recommends incremental refresh in status, doctor, and read diagnostics; projection-version and schema failures retain rebuild guidance.

Verification passed: `make check` with 1,835 Rust tests, Clippy, OpenCode plugin tests and packaging checks, plus [CI](https://github.com/btakita/tsift/actions/runs/34296745413). The spec and regression tests are updated. Built and installed locally; `tsift --version` reports `0.1.97`.

[Release v0.1.97](https://github.com/btakita/tsift/releases/tag/v0.1.97) includes Linux, macOS, and Windows binaries and checksums. [Publication](https://github.com/btakita/tsift/actions/runs/34296745682) passed: crates.io uploaded all 26 crates (`published=26 skipped=0`), verified individually at v0.1.97 and not yanked; npm confirms `opencode-tsift@0.1.97`.

The clean release worktree is `/tmp/tsift-issue-25`. The original checkout retains its preexisting uncommitted changes.


### Re: tsift compared with Graft — gpt-6

> **Queue prompt:**
>
> do [#comparetsiftgraft]


**Graft does not supersede tsift as a whole.** It is a credible alternative for repository orientation and retrieving relevant code for an agent. Its strongest contribution to tsift would be a simpler retrieval interface. tsift covers additional workflows—structural edits, durable evidence and memory, and compact session/test/log handling—that the inspected Graft implementation does not replace.

I reviewed Graft **0.17.0**, commit `3a85a2f`, against released tsift **0.1.97**, commit `4e8af21`, on September 10, 2026. This is a source comparison, not a measured performance comparison. The assessment uses the clean tsift release worktree, preserving the original checkout's existing changes.

| Area | Graft | Implication for tsift |
|---|---|---|
| Code retrieval | A question-oriented `ask` interface combines intent routing, lexical evidence, graph ranking, and source excerpts. | Make existing primitives easier to consume together; this is more specific than simply adding graph search. |
| Generated meaning | Optional deep indexing adds summaries and concepts. Per-symbol “crux” excerpts select decisive lines from actual source. | Add progressive expansion from summary to decisive excerpt to full symbol. |
| Graph precision | Tree-sitter extraction plus optional LSP call-edge enrichment. | An opportunity to improve selected call-resolution cases without building a type checker. |
| Change impact | Changed-line seeds, incoming dependency traversal, grouped modules, and evidence paths. | Extend the presentation and integration of tsift's existing `impact` command. |
| Agent integration | Six focused MCP tools cover finding code, file APIs, calls, exhaustive matches, maps, and freshness. | A small MCP interface could reduce the effort of choosing among tsift commands. |
| Broader workflows | Primarily repository context and navigation. | tsift's structural edits, graph evidence/memory, and digest envelopes remain separate reasons to use it. |

The retrieval and integration observations come from [Graft's ask implementation](https://github.com/trailhq/Graft/blob/3a85a2f40e283ac0c9b617d9a4bead18d73c3c70/src/ask/ask.ts) and [MCP tools](https://github.com/trailhq/Graft/blob/3a85a2f40e283ac0c9b617d9a4bead18d73c3c70/src/mcp/tools.ts). tsift's broader scope is documented in its [edit contract](https://github.com/btakita/tsift/blob/4e8af219a469b1f7c12378c546eff1bafaafcc5d/specs/ast-edits.md), [graph/evidence contract](https://github.com/btakita/tsift/blob/4e8af219a469b1f7c12378c546eff1bafaafcc5d/specs/graph.md), and [output envelopes](https://github.com/btakita/tsift/blob/4e8af219a469b1f7c12378c546eff1bafaafcc5d/specs/output-formats.md).

Several details matter when evaluating Graft's claims:

- **Its basic graph is deterministic.** LLM enrichment is optional; describing Graft as merely generated Markdown would miss its structural retrieval engine. Its file-based storage still includes a JSON graph and cached query sidecars. “No database” does not mean no maintained index. [Refresh implementation](https://github.com/trailhq/Graft/blob/3a85a2f40e283ac0c9b617d9a4bead18d73c3c70/src/graph/refresh.ts)
- **Freshness has separate layers.** Queries attempt structural refresh without calling a model. Lock contention or refresh errors can leave queries using the existing graph. Changed symbols retain summaries marked stale until enrichment runs. tsift navigation can also explicitly warn that a snapshot lags an active writer; neither tool should be described as universally fresh. Preserve tsift's operation-specific watermark/version checks and expose structural and summary freshness separately. [Graft summary invalidation](https://github.com/trailhq/Graft/blob/3a85a2f40e283ac0c9b617d9a4bead18d73c3c70/src/graph/enrich.ts)
- **LSP support is useful but bounded.** The inspected implementation picks one eligible installed server, enriches existing in-repository nodes, and falls back when unavailable. This is not proof of complete resolution across a mixed-language monorepo. [LSP enrichment](https://github.com/trailhq/Graft/blob/3a85a2f40e283ac0c9b617d9a4bead18d73c3c70/src/graph/lsp/enrich.ts), [server selection](https://github.com/trailhq/Graft/blob/3a85a2f40e283ac0c9b617d9a4bead18d73c3c70/src/graph/lsp/registry.ts)
- **The published benchmarks do not compare with tsift.** Graft reports improvements against a cold agent baseline, including a 50-instance SWE-bench evaluation. Those results support testing Graft's approach; they do not establish replacement, equal language coverage, or better large-workspace performance. [Published benchmark description](https://github.com/trailhq/Graft/blob/3a85a2f40e283ac0c9b617d9a4bead18d73c3c70/README.md#benchmark)

My recommended order is:

1. **Measure the actual gap — `#graftbenchmark`.** Use identical repositories, tasks, model, and context limits for baseline, tsift, and Graft. Separate cold setup, warm queries, dirty-tree updates, and optional model enrichment. Measure task correctness and missed related files alongside total tokens and latency.
2. **Build a question-oriented interface — `#grafttaskcontext`.** Route a task to existing search, graph, and source operations, returning a bounded source-backed context pack. Add thin MCP adapters and a compact repository map. Graft's file diversity and weak-scope filtering are worth testing; tsift already has graph ranking, so reuse that foundation. [Ranking and scope fusion](https://github.com/trailhq/Graft/blob/3a85a2f40e283ac0c9b617d9a4bead18d73c3c70/src/ask/fuse.ts)
3. **Add verified crux excerpts — `#graftcrux`.** Let optional enrichment select the branch, guard, formula, or state transition that explains a symbol. Slice the actual source, validate the span, and bind it to the body hash. Graft already implements per-symbol crux extraction; its proposed inline Markdown treatment should not be confused with shipped behavior. [Crux extraction](https://github.com/trailhq/Graft/blob/3a85a2f40e283ac0c9b617d9a4bead18d73c3c70/src/ai/crux.ts)
4. **Improve impact context — `#graftimpactcontext`.** Group affected production modules, show the path from each changed symbol, and separate test evidence. This extends [tsift's existing impact analysis](https://github.com/btakita/tsift/blob/4e8af219a469b1f7c12378c546eff1bafaafcc5d/packages/tsift-search/src/impact.rs), drawing on [Graft's blast analysis](https://github.com/trailhq/Graft/blob/3a85a2f40e283ac0c9b617d9a4bead18d73c3c70/src/blast/blast.ts).
5. **Prototype optional LSP edges — `#graftlspedges`.** Start with measured Rust/TypeScript resolution failures. Keep extraction usable without a server and retain edge provenance and coverage diagnostics. This fits a narrower scope than the full LSP-level type inference excluded by [tsift's current integration guidance](https://github.com/btakita/tsift/blob/4e8af219a469b1f7c12378c546eff1bafaafcc5d/specs/release-integration.md).

I would keep tsift's existing storage and evidence model while evaluating these additions. The recommendation is selective adoption, subject to a direct comparison. All five follow-ups are recorded as provisional `[recommended]` items; this comparison does not claim they are implemented.

Verification: tsift's full `make check` passed in the release worktree, and the [latest CI run](https://github.com/btakita/tsift/actions/runs/34296745413) passed. Graft was inspected from source; I did not run its model-backed benchmarks. Completed `#comparetsiftgraft`; top recommended backlog item: `#graftbenchmark`.


### Re: Graft benchmark — gpt-5

> **Queue prompt:**
>
> do [#graftbenchmark]


The benchmark supports adopting Graft's retrieval design, but it does not support replacing tsift.

I ran a zero-model retrieval benchmark using [Graft 0.18.0](https://github.com/trailhq/Graft/releases/tag/v0.18.0) at `de8456e` and [tsift 0.1.97](https://github.com/btakita/tsift/releases/tag/v0.1.97) at `4e8af21`. Graft 0.18.0 was released after the earlier source comparison, so this run uses the newer tagged release.

#### Method

The frozen corpus contains 12 hand-authored code-navigation questions: six about Graft and six about tsift. Each question has a source-verified set of one to three implementation files. Both tools received identical query text and returned at most eight results.

- Graft arm: deterministic `graft ask --source`, without deep summaries or LSP enrichment.
- tsift arm: `tsift search --strategy lexical --json`.
- Model and model cost: none; both arms made zero model calls and incurred $0 model cost.
- Correctness: target-file recall, complete target-set success, and mean reciprocal rank.
- Shared context budget: 12,000 output characters, approximately 3,000 tokens. A result counted as visible only when its path appeared before that cutoff.
- Timing: five warm repetitions per task; three full rebuilds and three one-file refreshes per repository.
- Isolation: each tool indexed a separate clean clone of each repository. Installation time was excluded.

#### Results

| Metric, 12 tasks | tsift | Graft |
|---|---:|---:|
| Target-file Recall@8, unbounded | 55.6% | **75.0%** |
| Target-file Recall@8, within budget | 47.2% | **63.9%** |
| Complete target sets, unbounded | 4/12 | **7/12** |
| Complete target sets, within budget | 3/12 | **5/12** |
| Mean reciprocal rank | 0.389 | **0.747** |
| Median warm query | **21.7 ms** | 196.0 ms |
| Mean raw response | **8.87 KB** | 14.12 KB |
| Responses over 12 KB | **3/12** | 10/12 |
| Approximate raw tokens per task | **2,218** | 3,529 |

The aggregate result is influenced by a strong home-repository advantage:

| Repository | tsift budgeted recall | Graft budgeted recall | Complete sets |
|---|---:|---:|---:|
| Graft | 52.8% | **77.8%** | tsift 1/6, Graft 3/6 |
| tsift | 41.7% | **50.0%** | 2/6 each |

On the foreign tsift repository, Graft's budgeted recall advantage was 8.3 percentage points, while both tools completed the same number of target sets. That is encouraging for Graft's question-oriented ranking, but too small a sample to claim general superiority. The corpus also scores implementation files: tsift sometimes ranked architectural specs ahead of source, which is useful for explanation but counted as a miss here. Graft indexed a narrower code-only surface in the tsift repository.

Latency shows the opposite tradeoff. With automatic freshness checks enabled and no changes, median queries were 16.6/25.6 ms for tsift on the Graft/tsift repositories and 164.9/208.4 ms for Graft. After one source file changed, three-run medians were 42.7/40.8 ms for tsift and 448.0/528.7 ms for Graft.

Full rebuilds split by repository:

| Repository | tsift rebuild | Graft rebuild |
|---|---:|---:|
| Graft | **2.03 s** | 3.33 s |
| tsift | 2.21 s | **1.30 s** |

Cache size also varied. On Graft, tsift used 7.95 MB and Graft 10.63 MB. On tsift, tsift used 73.62 MB and Graft 23.84 MB. These sizes are not feature-equivalent: tsift stores broader document, AST, graph, and freshness data, while Graft's structural pass covered 127 code files in the tsift repository.

#### Decision

Graft's strongest measured advantage is ranking the first relevant implementation file and collecting related files. tsift's strongest measured advantages are query latency, refresh latency, and compact responses. The useful path is to add a Graft-style task-context facade over tsift's existing index and graph rather than migrate to Graft's storage or replace tsift.

I refined `#grafttaskcontext` with benchmark-based acceptance criteria: retain explicit source-versus-document modes, exceed 50% budgeted Recall@8 on the foreign-repository slice, stay within 12,000 characters, and target a warm median below 50 ms. The frozen corpus should be expanded before treating those thresholds as a release gate.

Limitations: this benchmark measures retrieval, not patch correctness; uses 12 author-selected questions; uses a character cutoff rather than a provider tokenizer; excludes model-enriched summaries, LSP edges, installation time, and end-to-end agent behavior. Graft's richer inline source explains part of its larger response. A model-backed SWE-bench comparison would answer a different and substantially more expensive question.

Verification: all benchmark commands completed successfully. The root `make check` passed. The latest root [Agent Doc CI run](https://github.com/btakita/agent-loop/actions/runs/34522009943) predates this benchmark; its tmux job passed, while its check job failed in the agent-doc dev-harness on missing Node typings/local Lazily packages. No product source changed in this cycle. Completed `#graftbenchmark`; the next queued recommendation is `#grafttaskcontext`.

### Re: Response

Implemented `#grafttaskcontext` as a working prototype in the clean tsift worktree `/tmp/tsift-issue-25` and committed it as `4456d9c feat: add task context and MCP retrieval facade`.

The new `tsift task-context <question>` command combines indexed FTS file ranking with symbol evidence, deduplicates and reranks by file, emits bounded source excerpts, and supports explicit `source`, `docs`, and `mixed` modes. Its JSON contract includes relevance, weak-match abstention, coverage, freshness, diagnostics, and exact serialized byte accounting. The default path inspects and refreshes the index. `--no-autoindex` preserves the sealed snapshot for the low-latency path and reports freshness as `unknown` rather than claiming an unverified snapshot is fresh.

The new `tsift mcp` stdio server exposes five focused adapters: `tsift_task_context`, `tsift_search`, `tsift_source`, `tsift_graph`, and `tsift_freshness`. It supports JSON-RPC `initialize`, `ping`, `tools/list`, and `tools/call`, bounds tool output, preserves UTF-8 boundaries, and returns `structuredContent` when bounded output remains valid JSON. The task-context MCP adapter also exposes the autoindex choice.

Validation on the frozen 12-task Graft comparison corpus, with top 8, a 12,000-byte cap, six invocations per task, and the first timing discarded:

- Overall Recall@8: **93.1%**; 10/12 tasks returned every gold file.
- Graft repository slice: **86.1%** Recall@8; 4/6 complete tasks.
- tsift repository slice: **100%** Recall@8; 6/6 complete tasks.
- Overall warm median: **10.0 ms**; Graft slice 5.4 ms; tsift slice 28.7 ms.
- Budget compliance: **12/12** responses under 12,000 bytes; largest response 7,422 bytes.
- An unrelated nonsense query returned `relevance: none`, `abstained: true`, and zero hits.
- Separate live checks proved docs mode returned only documentation and source mode excluded documentation.
- An end-to-end MCP call returned structured task context with the expected `packages/tsift-search/src/impact.rs` first.

Verification passed: full repository `make check`, `cargo clippy -p tsift-cli --all-targets -- -D warnings`, five task-context unit tests, two MCP unit tests, `git diff --check`, and the public-repo private-name scan. The latest upstream tsift GitHub Actions run is the successful `v0.1.97` Release run 34296745682; this local branch has not been pushed, so no CI run contains `4456d9c` yet.
<!-- agent:boundary:b8ec92e9:tsift -->
<!-- /agent:exchange -->

## Queue

<!-- agent:queue preset="#spec-test-build-install-commit-push" priority go -->
- ~~do [#graftbenchmark]~~
- 🚧 do [#grafttaskcontext]
- do [#graftcrux]
- do [#graftimpactcontext]
- do [#graftlspedges]
<!-- /agent:queue -->
<!--
https://github.com/chopratejas/headroom
-->

## Backlog

<!-- agent:backlog priority queue -->
- [ ] 🚧 [#grafttaskcontext] [recommended] Prototype a question-oriented task-context interface and thin MCP adapters over existing tsift search, graph, source, and freshness primitives. Validate on the frozen Graft comparison corpus: improve foreign-repo budgeted Recall@8 beyond 50%, preserve explicit source-versus-doc modes, keep output within 12,000 characters, target warm median latency below 50 ms, and retain weak-match abstention and freshness evidence.
- [ ] [#graftcrux] [recommended] Add optional source-verified crux excerpts to tsift summaries and symbol context, anchored to symbol spans and body hashes with explicit stale-state handling.
- [ ] [#graftimpactcontext] [recommended] Extend existing tsift impact output with grouped production modules, changed-symbol evidence paths, and separate test signals; expose bounded context to post-edit integrations.
- [ ] [#graftlspedges] [recommended] Prototype optional LSP call-edge enrichment using installed servers, preserving extractor fallback, provenance, confidence, coverage diagnostics, and tsift freshness contracts; validate Rust and TypeScript resolution precision.
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

<!-- agent:notes -->
• Ran tsift rewrite --run 'git log -5 --oneline --decorate'
└ tsift rewrite: no rewrite: no supported tsift rewrite matched this command; `--run` executes only rewritten commands; run the original command directly if intended

#spec-test-build-install-commit-push
---
route duplicate prompt residue guard failed: failed to parse components: unclosed component: `agent:notes` without matching close
<!-- /agent:notes -->
