---
agent_doc_session: tsift-v0.1
resume: 019dd526-7958-77e3-9a41-92b8944fc550
agent: codex
claude_model: claude-opus-4-6
agent_doc_format: template
agent_doc_write: crdt
claude_args: --dangerously-skip-permissions
codex_args: -s danger-full-access
prompt_presets:
  '#agent-doc-bug': Please create a plan for agent-doc to fix this issue. Add to the backlog of agent-loop/tasks/agent-doc/agent-doc-bugs2.md
  '#code-review': Please review tsift codebase. Inspect the architecture and look for edge cases. Isolate concurrency issues and other workflow bugs. Let's find root causes to fix. '#follow-up-backlog'
  '#next-steps': Any follow-up items to place in the backlog? Please ensure the backlog is in priority order. Print the item id of the top of the backlog.
  '#spec-test-build-install-commit-push': update spec + tests. build + install for local testing. commit + push
  '#spec-test-commit-push': update spec + tests. commit + push
---

## Status

<!-- agent:status patch=replace -->
Project created. Repo: https://github.com/btakita/tsift. Submodule: src/tsift. Spec extracted to src/tsift/SPEC.md.
<!-- /agent:status -->

## Spec

See [src/tsift/SPEC.md](../../src/tsift/SPEC.md) for the AST-Aware Code RAG spec.

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc65443c45f7b49f5e6ace54e3f6bafe4077ade57924422b5eb3baf4dbd23c-20260513-015410.md`*

Compacted content:
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc6544...
❯ - Trailing prompt/context: Please review https://arxiv.org/abs/2605.05242. What are the implications for tsift?
❯ Please review https://arxiv.org/abs/2605.05242. What are the implications for tsift?
### Re: arXiv 2605.05242 implications — gpt-5

I reviewed [arXiv:2605.05242](https://arxiv.org/abs/2605.05242), "Beyond Semantic Similarity: Rethinking Retrieval for Agentic Search via Direct Corpus Interaction," plus the current tsift spec/code. The paper is more validation than pivot for tsift: it argues that capable agents often need a higher-resolution corpus interface than a fixed top-k retriever, and that direct corpus interaction through `grep`/`rg`, file reads, shell pipelines, and lightweight scripts can beat strong sparse/dense/reranking baselines on agentic search tasks.

The strongest implication is that tsift should stay centered on agent-controllable corpus operations, not on becoming a hidden semantic retriever. Existing tsift directions already line up with that: `search --exact` maps to `rg -F`, literal identifier/path-like queries auto-promote to exact search, scoped/federated workspace selection protects corpus boundaries, `--budget`/`--envelope` keeps observations bounded, and `explain`/graph/context-pack surfaces provide local inspection instead of only a ranked list.

What the paper adds is a sharper product metric: retrieval interface resolution. tsift should measure whether an agent can narrow from broad clue to exact span, compose weak constraints, inspect local context, and launch the next search from surfaced evidence. That is different from only measuring top-k recall or NDCG. For code work, the useful benchmark is not "did the right file appear in the first ten results?" but "how many bounded tool steps did it take to isolate the symbol, call edge, failing test, or edit site?"

Concrete implications for the roadmap:

- Keep exact search first-class. The paper's tool traces are dominated by `rg | head`, chained `rg`, and local context peeking. tsift should make exact and chained narrowing cheap, bounded, and pleasant.
- Treat BM25/vector/hybrid as accelerators, not the primary abstraction. They help locate anchors, but the agent still needs raw corpus operations once it has a clue.
- Promote result handles and expansion commands. Every bounded preview should make the next narrowing operation obvious, because DCI wins by iterative refinement.
- Add DCI-style evals. Compare `search --exact`, lexical, hybrid, `explain`, and digest/context-pack workflows on multi-hop repo tasks using tool calls, latency, token volume, and localization quality.
- Add breadth guards. The paper reports that DCI scales well in search depth but gets expensive as corpus breadth grows. tsift should warn when a query is too broad and suggest narrower next commands before an agent burns budget.
- Keep context management pragmatic. The paper's best results are not from preserving everything verbatim; selective truncation/compaction beats indiscriminate accumulation. tsift's budgeted envelopes and digest surfaces are directly aligned with that.

Bottom line: tsift should frame itself as a high-resolution corpus interaction layer for agents. Semantic retrieval can remain a strategy, but the differentiator should be controlled, composable, inspectable search over live local corpora.
❯ Agreed. do [#686r]. spec-test-build-install-commit-push
### Re: DCI benchmark — gpt-5

Done. Implemented [#686r] and pushed it.

tsift now has `tsift dci-benchmark --fixture <path>` with a checked-in `fixtures/dci-search-benchmark.json` seed benchmark. It compares `exact_chained_rg`, `lexical_bm25`, and `hybrid` across multi-hop repo/code localization tasks and reports localization rate, average tool calls, latency, and estimated token budget. I also documented the benchmark in `SPEC.md` and added CLI/integration coverage.

I had to fix the remote CI surface while pushing: the workflow now checks out sibling `tagpath`, installs `ripgrep` for exact-search tests, and release verify uses the same layout. I also fixed the Rust 1.95 Clippy lints that GitHub surfaced.

Verification:
- `cargo test dci_benchmark`
- `make check`
- `cargo clippy --all-targets -- -D warnings`
- `cargo build`
- `cargo install --path .`
- `tsift dci-benchmark --fixture fixtures/dci-search-benchmark.json --compact`
- GitHub CI `btakita/tsift` run `25774327358` passed (`clippy + test`). There is no tmux CI leg in the tsift workflow.

Pushed:
- tsift: `35b087b`
- agent-loop submodule pointer: `06710a49d`
<!-- agent:boundary:cfdcd0cc -->
<!-- /agent:exchange -->
###

## Pending / Not Built

<!-- agent:backlog -->
- [ ] [#s1k1] Add a composable search workflow surface or documented recipes that preserve result handles across search --exact, search, explain, summarize, and digest expansion steps.
- [ ] [#v6y3] Add breadth/scale guards to search envelopes: high-hit warnings, corpus-size/tool-budget signals, and concrete next-narrowing commands before agents fan out too broadly.
<!-- /agent:backlog -->

<!-- agent:icebox -->
- [ ] [#9jc8] graphify integration: LLM-based entity extraction for semantic layer beyond AST
- [ ] [#zv4m] graphify integration: HTML visualization of symbol/dependency graphs
- [ ] [#rd2f] graphify integration: semantic similarity between concepts (embedding-based)
<!-- /agent:icebox -->

## Completed / Reaped

<!-- agent:done -->
- 2026-05-12 [#686r] Add a DCI-style tsift benchmark: compare exact/chained rg, lexical BM25, and hybrid search on multi-hop code/repo tasks, tracking localization, tool calls, latency, and token budget.
<!-- /agent:done -->

## Completed / Reaped

<!-- agent:pending-done -->
- 2026-05-09 [#ctxp] Add context-pack to the token-savings fixture/regression, including diff/test/log/next-context inputs and a fail-under threshold for its envelope preview.
- 2026-05-10 [#real] Add a real-session token-savings benchmark from recent tsift/agent-doc transcripts, covering session-review and context-pack paths that dominate actual prompt volume.
- 2026-05-10 [#budg] Tune the response-budget presets and auto caps against measured token-savings data so envelope previews stay compact without hiding required follow-up commands.
<!-- /agent:pending-done -->

<!--
-->
