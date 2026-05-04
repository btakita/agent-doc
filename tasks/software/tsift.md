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

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc65443c45f7b49f5e6ace54e3f6bafe4077ade57924422b5eb3baf4dbd23c-20260503-030255.md`*

❯ Icebox:
❯ - [ ] [#9jc8] graphify integration: LLM-based entity extraction for semantic layer beyond AST
❯ - [ ] [#zv4m] graphify integration: HTML visualization of symbol/dependency graphs
❯ - [ ] [#rd2f] graphify integration: semantic similarity between concepts (embedding-based)
❯ #code-review
### Re: code review — gpt-5

1. High: timed search now misdiagnoses a concurrently removed or reset index as a "fresh" search root, which points the user away from the actual recovery step. `collect_stale_search_targets()` only records `SearchIndexState::Stale` and drops `Missing` entirely ([src/tsift/src/main.rs](/home/brian/work/btakita/agent-loop/src/tsift/src/main.rs:3341)), so `search_timeout_message()` falls through to the fresh-root hint whenever the index vanishes during the worker timeout window ([src/tsift/src/main.rs](/home/brian/work/btakita/agent-loop/src/tsift/src/main.rs:3427), [src/tsift/src/main.rs](/home/brian/work/btakita/agent-loop/src/tsift/src/main.rs:8023)). I reproduced it locally by indexing a temp repo, starting `tsift search --timeout 1`, deleting `.tsift/index.db` while the worker was sleeping, and observing: `The search root looks fresh, so reindexing is unlikely to help.` That is the same concurrency class this patch is trying to fix; a missing target after timeout should produce a rebuild instruction and needs a regression test next to `search_timeout_reports_reindex_when_index_turns_stale_during_worker_run()`.

❯ I ran `cargo test search_timeout`; the current tests cover the fresh-root and stale-after-mutation cases, but not the missing-index race above.
❯ do [#29jg]. spec-test-build-install-commit-push
### Re: #29jg spec-test-build-install-commit-push — gpt-5

Fixed the timeout re-check so `tsift search --timeout ...` now treats both stale and missing indexes as rebuild-needed instead of falling back to the fresh-root hint when `.tsift/index.db` disappears mid-run. I added regression coverage in the direct timeout helper and the compiled CLI search surface, and updated `SPEC.md` plus `VERSIONS.md` to document the stale-or-missing timeout behavior.

Verification in `src/tsift`: `cargo test` passed (`452` unit tests + `54` exit-code tests), `cargo build --release` passed, and `cargo install --path . --force` replaced `~/.cargo/bin/tsift`. The change is committed and pushed in `src/tsift` as `07df133` (`fix: report missing search indexes after timeout`) on `origin/main`.
❯ I saw this error in claudescore-3.md. Is this causing problems with the agent?
```
• Ran tsift explain run_sync
  └ Symbol `run_sync` not found in index.
    (Checking call graph for references...)
    … +4 lines (ctrl + t to view transcript)
    Callees (0):
      (none)
```
### Re: run_sync lookup — gpt-5

Not by itself. I reproduced the exact output from `/home/brian/work/btakita/agent-loop/src/session-share`, and it happens because `tsift` indexes that repo root, not the whole parent workspace. In that repo, `run_sync` is outside the indexed tree, so `tsift explain run_sync` legitimately returns `Symbol \`run_sync\` not found in index.`

From the workspace root `/home/brian/work/btakita/agent-loop`, the same command resolves normally to `src/corky/src/search/ragie.rs:97` and returns callers/callees. So this is a scope/index-root mismatch, not evidence that the agent is malfunctioning. The practical effect is narrower context: if the agent needed that symbol, it would miss the cross-repo definition until it reruns from the broader root or reindexes the correct workspace.
`agent-doc plan` blocked this cycle because it found no live document changes relative to the current snapshot, so I did not write or finalize a new response.

❯ The earlier preflight briefly saw a trailing line that looked like `Why was this symbol search attempted? spec-test-build-install-commit-push`, but by the time the binary planning step ran, that change was no longer present in the document. If you want that answered in the session doc, add the prompt back to [tasks/software/tsift.md](/home/brian/work/btakita/agent-loop/tasks/software/tsift.md) and rerun `agent-doc`.
❯ Why was this symbol search attempted? spec-test-build-install-commit-push
### Re: why that symbol lookup happened — gpt-5

It does not look like `#spec-test-build-install-commit-push` caused the lookup. That tag belongs to this `tsift.md` session. The quoted `tsift explain run_sync` line came from the separate `claudescore-3.md` session, whose current task is `do [#pfqcal2]. spec-test-benchmarks-news-commit-push`, not this tsift preset.

I could not recover the exact prompt that triggered `tsift explain run_sync`; the retained markdown only preserves the command output fragment, not the preceding reasoning. The most likely explanation is routine code navigation: `src/session-share/AGENTS.md` explicitly recommends `tsift explain <symbol>` for caller/callee inspection, and the agent likely saw a `run_sync` identifier while inspecting code or transcript material and probed it. The lookup then failed because it ran from `src/session-share`, whose index does not include the sibling workspace code where `run_sync` actually resolves.

So the short version is: this was most likely an ad hoc symbol-inspection attempt in the other session, and the failure was caused by index scope, not by the `spec-test-build-install-commit-push` directive itself.
❯ I saw this error. Why was there no reindex?:
```
• Ran tsift search "claudescore-3"
  └ Error: tsift search timed out after 30s (strategy: lexical). The search root looks fresh, so reindexing is unlikely
    to help. Re-run with `--timeout 0` to disable the timeout, narrow `--path` / `--scope`, or try a different
    strategy.
```
### Re: why there was no reindex — gpt-5

Because that timeout path is intentionally the opposite case. `tsift search` autoindexes before the search starts when the index is missing or stale, via `precheck_search_indexes()` in [src/tsift/src/main.rs](/home/brian/work/btakita/agent-loop/src/tsift/src/main.rs:3493). After a timeout, it re-checks the same targets with `search_timeout_message()` and only tells you to rebuild if the index became stale or disappeared while the worker was running ([src/tsift/src/main.rs](/home/brian/work/btakita/agent-loop/src/tsift/src/main.rs:3451), [src/tsift/src/main.rs](/home/brian/work/btakita/agent-loop/src/tsift/src/main.rs:8050)).

❯ So when you saw `The search root looks fresh, so reindexing is unlikely to help`, tsift had already decided: the index was present and looked current, and the problem was the lexical search taking longer than 30s, not index drift. In other words, this was treated as a slow/broad query (`"claudescore-3"` over that root), not a stale-index case. A reindex would only have been suggested if the post-timeout re-check found `index is stale` or `index is missing` instead.
❯ Why was the claudescore-3 slow? Is there a way to make this query fast? A simple lookup should be fast.
### Re: claudescore-3 search slowness — gpt-5

Yes: a simple literal lookup *should* be fast. The current `tsift search` lexical path is not doing a cheap literal lookup, though. In `search_command`, `tsift` uses the SQLite index for the symbol side, but it still sends the text query through the upstream `sift` engine via `run_search_with_timeout(...)` ([src/tsift/src/main.rs](/home/brian/work/btakita/agent-loop/src/tsift/src/main.rs:3505), [src/tsift/src/main.rs](/home/brian/work/btakita/agent-loop/src/tsift/src/main.rs:3544)). That engine instance is built with `Sift::builder().build()` and no cache dir, so each lexical search reloads the corpus and rebuilds BM25 over the live file tree instead of reusing a persisted search cache ([src/tsift/Cargo.toml](/home/brian/work/btakita/agent-loop/src/tsift/Cargo.toml:21), [/home/brian/.cargo/git/checkouts/sift-4be901efc2828196/df22d7d/src/facade.rs](/home/brian/.cargo/git/checkouts/sift-4be901efc2828196/df22d7d/src/facade.rs:99), [/home/brian/.cargo/git/checkouts/sift-4be901efc2828196/df22d7d/src/facade.rs](/home/brian/.cargo/git/checkouts/sift-4be901efc2828196/df22d7d/src/facade.rs:292), [/home/brian/.cargo/git/checkouts/sift-4be901efc2828196/df22d7d/src/search/corpus.rs](/home/brian/.cargo/git/checkouts/sift-4be901efc2828196/df22d7d/src/search/corpus.rs:76), [/home/brian/.cargo/git/checkouts/sift-4be901efc2828196/df22d7d/src/search/application.rs](/home/brian/.cargo/git/checkouts/sift-4be901efc2828196/df22d7d/src/search/application.rs:607)).

The query shape makes it worse. The default `lexical` plan is BM25-only, not phrase lookup, and tokenization splits on non-alphanumerics, so `claudescore-3` becomes `claudescore` plus `3` rather than one exact token ([src/tsift/src/main.rs](/home/brian/work/btakita/agent-loop/src/tsift/src/main.rs:3544), [/home/brian/.cargo/git/checkouts/sift-4be901efc2828196/df22d7d/src/search/domain.rs](/home/brian/.cargo/git/checkouts/sift-4be901efc2828196/df22d7d/src/search/domain.rs:1704), [/home/brian/.cargo/git/checkouts/sift-4be901efc2828196/df22d7d/src/search/domain.rs](/home/brian/.cargo/git/checkouts/sift-4be901efc2828196/df22d7d/src/search/domain.rs:2041), [/home/brian/.cargo/git/checkouts/sift-4be901efc2828196/df22d7d/src/search/adapters/mod.rs](/home/brian/.cargo/git/checkouts/sift-4be901efc2828196/df22d7d/src/search/adapters/mod.rs:464)). In `src/session-share`, that root still contains `1367` tracked files, and `109` files literally contain `claudescore-3`; from the full `agent-loop` root it is `38016` files and `152` exact-string hits. So the timed worker is paying full corpus-load and BM25-build cost for a query that is also broader than it looks.

So the short answer is: the slowdown is expected with the current implementation, and it is not an index-freshness problem. Fast *today* means either narrowing the root (`--path`, `--scope`) or using a literal tool like `rg` when you already want exact text. If you want `tsift search "claudescore-3"` itself to become fast, the likely fix is to give the `sift` engine a stable cache dir and/or add an exact-literal path for hyphenated identifier-like queries instead of routing them through uncached BM25.
<!-- agent:boundary:827f008f -->
<!-- /agent:exchange -->

<!--
---
Can tsift use rtk to compress tool call output in the context?
-->

## Pending / Not Built

<!-- agent:backlog -->
<!-- /agent:backlog -->

<!-- agent:icebox -->
- [ ] [#9jc8] graphify integration: LLM-based entity extraction for semantic layer beyond AST
- [ ] [#zv4m] graphify integration: HTML visualization of symbol/dependency graphs
- [ ] [#rd2f] graphify integration: semantic similarity between concepts (embedding-based)
<!-- /agent:icebox -->

<!--
-->
