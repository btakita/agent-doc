---
agent_doc_session: tsift-v0.1
agent: codex
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

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc65443c45f7b49f5e6ace54e3f6bafe4077ade57924422b5eb3baf4dbd23c-20260531-024318.md`*

Compacted content:
- Archived 2 response topic(s): Can tsift now replace claude-mem?; What are #next-steps to replace claude-mem?
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc6544...

❯ Archived 2 response topic(s): Can tsift now replace claude-mem?; What are #next-steps to replace claude-mem?

### Re: queued claude-mem migration — gpt-5

I started the queued migration batch with the requested `#spec-test-build-install-commit-push` preset, but the binary-owned `agent-doc orchestrate` path stalled inside its spawned `codex exec` child for more than ten minutes after running the `src/agent-doc` workspace tests. I interrupted that stuck handoff so the session document would not remain in an open orchestration state.

No backlog item is complete yet. `#tsiftmemimportall`, `#tsiftmemsemanticgate`, `#tsiftmemparityeval`, `#tsiftmemcutover`, and `#tsiftmemretire` should stay open until a fresh cycle can execute them from a clean preflight/plan contract.

I also saw the preflight warning that the post-exchange `#next-steps` note is preserved as a non-executable comment; move it into `agent:exchange` or `agent:queue` if it should run.

### Re: #tsiftmemimportall — gpt-5

> **Queue prompt:**
>
> do [#tsiftmemimportall]


Completed the full supported `claude-mem` import work.

Implemented and committed in `src/tsift`:
- `tsift memory import-claude-mem` now supports `--all` for uncapped supported-table imports while keeping the default 1000-row per-table cap unless `--all` is set.
- Import reports now include explicit per-table and total reconciliation for `observations`, `session_summaries`, and `user_prompts`: source rows, read/planned rows, imported rows, already-present rows, skipped rows, and `complete`.
- Apply mode now uses one transactional bulk insert path instead of per-event autocommit; the interrupted live import showed the old path was too slow for the 50k-row migration.
- Large JSON reports cap `event_ids` at 100 and expose `event_ids_total` plus `event_ids_truncated`, so full imports stay count-proof oriented without enormous output.
- README, SPEC, and `tsift-memory` docs now describe the full import and compact report contract.

Live migration proof for `/home/brian/work/btakita/agent-loop/.tsift/memory.db` using the installed `tsift` binary:
- `planned_events=50625`
- `total_source_rows=50625`
- `total_read_events=50625`
- `imported_events=2`
- `already_present_events=50623`
- `total_skipped_source_rows=0`
- `complete=true`
- `event_ids_total=50625`, `event_ids_truncated=true`

Verification completed:
- `cargo test -p tsift-memory claude_mem` passed.
- `make check` passed: clippy, full Rust workspace tests, opencode plugin tests, and npm publish dry run.
- `cargo build --workspace` passed.
- `cargo install --path . --force` replaced `/home/brian/.cargo/bin/tsift`.
- `tsift --version` reports `tsift 0.1.62`.
- Latest GitHub Actions `CI` run `26696235528` on `main` is green; this repo's CI jobs are `clippy + test` and `live Convex graph acceptance`, with no tmux-specific job listed.

Commits:
- `d9e3691 feat(memory): reconcile full claude-mem import` in `src/tsift`, pushed to `btakita/tsift` `main`.
- `763f85dcf chore: update tsift submodule` in the root superproject; root push follows this document closeout.

The remaining queued items stay in order: `#tsiftmemsemanticgate`, `#tsiftmemparityeval`, `#tsiftmemcutover`, and `#tsiftmemretire`.

### Re: #tsiftmemsemanticgate — gpt-5

> **Queue prompt:**
>
> do [#tsiftmemsemanticgate]


Done. I treated the recursive `agent-doc` re-entry warning as a closed empty cycle and continued the active response manually from preflight/plan, preserving the queue contract.

What changed:

- Fixed native `.tsift/memory.db` graph projection IDs so multiple tsift-memory events from the same session document no longer collide during `graph-db refresh`.
- Taught graph-db semantic readiness to accept materialized `semantic_concept` / `semantic_entity` rows from tsift-memory graph records, not only LLM summary-cache rows.
- Updated the spec to document that imported/native memory semantic rows can satisfy graph-db semantic readiness when the summary cache is empty.
- Added regression coverage for repeated native memory source refs and the summary-empty readiness path.

Live evidence from the installed `tsift 0.1.62` binary:

- `tsift graph-db --path /home/brian/work/btakita/agent-loop --json refresh` now completes with `status=current`, `readiness.status=ready`, `reason=semantic_rows_available`, `nodes=375141`, and `edges=648673`.
- `tsift graph-db --path /home/brian/work/btakita/agent-loop --json related 'claude-mem import tsift memory'` returns `readiness.status=ready`, `seed_count=5`, `node_count=18`, `edge_count=15`, including tsift-memory hits for the claude-mem replacement prompts and imported observations.
- I attempted `tsift summarize --extract . --path /home/brian/work/btakita/agent-loop --json`; it processed `0` files because `ANTHROPIC_API_KEY` is not set, so no LLM summary-cache rows were populated in this environment. The semantic gate is satisfied by refreshed tsift-memory semantic rows instead of failing closed with `summary_cache_empty`.

Verification passed:

- `cargo fmt`
- `cargo test -p tsift-cli traversal_projection_materializes_tsift_memory_rows`
- `cargo test -p tsift-cli graph_db_related_reports_summary_extract_gate_when_summary_cache_empty`
- `make check`
- `cargo build --workspace`
- `cargo install --path . --force`
- `tsift --version`
- GitHub Actions `CI` for `e3106ba` passed both jobs: `live Convex graph acceptance` and `clippy + test`. The only annotations were the existing Node.js 20 actions deprecation warning and the live Convex URL configuration note.

Commits:

- `src/tsift`: `e3106ba fix(graph-db): accept memory semantic readiness` pushed to `btakita/tsift` `main`.
- parent repo: `4a8255395 chore: update tsift submodule` created; I will push it after this document closeout commit lands.

Remaining queue items are `#tsiftmemparityeval`, `#tsiftmemcutover`, and `#tsiftmemretire`.

### Re: #tsiftmemparityeval — gpt-5

> **Queue prompt:**
>
> do [#tsiftmemparityeval]


Done. I reran the queued direct entrypoint, but it stalled after synthesizing the queue prompt, so I killed that process, repaired the stale agent-doc preflight lock with `agent-doc write --commit`, restored the exchanged boundary to the committed value, and continued from a clean preflight/plan for `#tsiftmemparityeval`.

What changed:

- Added an explicit `memory_retrieval_gate` to `tsift dci-benchmark --json` when a fixture declares `claude_mem_api`, `tsift_session_review_context_pack`, and `graph_db_related`.
- The gate requires each tsift candidate to meet or exceed the claude-mem average useful-hit baseline and have fewer zero-output failures, with equal zero failures accepted when the claude-mem baseline is already zero.
- Added human and compact CLI output for the gate decision.
- Extended the memory retrieval fixture test to assert the cutover gate passes, and added a regression test that blocks a candidate with worse useful hits or zero-output behavior.
- Updated `SPEC.md` to document the memory retrieval cutover gate.

Installed-binary evidence:

- `tsift dci-benchmark --fixture fixtures/memory-retrieval-eval.json --json` reports `top_strategy=graph_db_related` and `memory_retrieval_gate.decision=pass`.
- Gate rows: `tsift_session_review_context_pack` has `avg_useful_hits=3.5`, `zero_output_failures=0`; `graph_db_related` has `avg_useful_hits=3.75`, `zero_output_failures=0`; the claude-mem baseline is `min_avg_useful_hits=0.25`, `max_zero_output_failures=2`.

Verification passed:

- `tsift index .`
- `cargo fmt`
- `cargo test -p tsift-quality memory_retrieval_gate`
- `cargo test -p tsift-quality dci_benchmark::tests::supports_memory_retrieval_metrics`
- `cargo test -p tsift-cli dci_benchmark_summarizes_memory_retrieval_eval_fixture`
- `cargo run -p tsift-cli -- dci-benchmark --fixture fixtures/memory-retrieval-eval.json --json`
- `make check`
- `cargo build --workspace`
- `cargo install --path . --force`
- `tsift --version` -> `tsift 0.1.62`
- GitHub Actions `CI` for `d06ded6` passed both jobs: `live Convex graph acceptance` and `clippy + test`.

Commits:

- `src/tsift`: `d06ded6 feat(dci): gate memory retrieval cutover` pushed to `btakita/tsift` `main`.
- parent repo: `61cfe3feb chore: update tsift submodule` created; I will push it after this document closeout commit lands.

Remaining queue items are `#tsiftmemcutover` and `#tsiftmemretire`.

<!-- agent:boundary:39d1c2fc -->
<!-- /agent:exchange -->
###
<!--
What are #next-steps to improve token efficiency along with graph + neighborhood performance & effectiveness.
---
What are #next-steps to make the graph faster?
---
What are #next-steps to make the graph + neighborhoods faster + more effective?
-->

<!-- agent:queue auto -->
preset #spec-test-build-install-commit-push
- ~do [#tsiftmemimportall]~
- ~do [#tsiftmemsemanticgate]~
- ~do [#tsiftmemparityeval]~
- do [#tsiftmemcutover]
- do [#tsiftmemretire]
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog -->
- [ ] [#tsiftmemcutover] Replace live memory retrieval call sites and operator instructions that point at direct claude-mem or /mem-search with tsift-memory / graph-db retrieval, while keeping claude-mem as a fallback during the first cutover pass.
- [ ] [#tsiftmemretire] Retire claude-mem only after full import, semantic retrieval, and parity eval are green; keep a documented rollback path until at least one normal session cycle succeeds without direct claude-mem reads.
<!-- /agent:backlog -->

## Review

<!-- agent:review -->
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
- [ ] [#npm-auth-setup] Configure NPM_TOKEN secret + TSIFT_ENABLE_NPM_PUBLISH=true variable in GitHub repo settings
- [ ] [#npm-publish-verify] After auth configured, tag release and verify publish-opencode-plugin CI job succeeds and opencode-tsift appears on npmjs.org
- [ ] [#full-e2e-verify] In fresh project: opencode plugin opencode-tsift && opencode then /tsift-status. On success mark #e2e-install done.
<!-- /agent:icebox -->

## Completed / Reaped

<!-- agent:done archive=tasks/software/tsift.done.md -->

<!-- completed work archived in tasks/software/tsift.done.md -->
<!-- /agent:done -->
