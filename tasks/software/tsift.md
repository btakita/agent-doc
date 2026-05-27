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
  '#next-steps': Any follow-up items to place in the backlog? Please ensure the backlog is in priority order. Print the item id of the top of the backlog.
  '#spec-test-build-install-commit-push': update spec + tests. build + install for local testing. commit + push
  '#spec-test-commit-push': update spec + tests. commit + push
queue_active: true
---

## Status

<!-- agent:status patch=replace -->

Completed #gcommqual; verified community member tagpath context is already implemented and pushed in tsift. Local make check, cargo build, and cargo install pass.
<!-- /agent:status -->

## Spec

See [src/tsift/SPEC.md](../../src/tsift/SPEC.md) for the AST-Aware Code RAG spec.

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc65443c45f7b49f5e6ace54e3f6bafe4077ade57924422b5eb3baf4dbd23c-20260527-065332.md`*

Compacted content:
- Archived 6 response topic(s): graph + neighborhood next steps; #gneighqual; closeout repair; 3 more
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc6544...
### Re: #gfullhot — gpt-5

Completed `#gfullhot`.

I changed the full-projection cache key in `src/tsift` so it is based on stable indexed inputs: symbol rows, call-edge rows, route rows, and semantic summary rows. The full-projection key now ignores provider-neutral `file_state` path/mtime churn and agent-doc session markdown churn, while still keeping session markdown in the bounded real-dataset evidence path. Cache lookup details now print the component hashes and source watermark detail so backend-eval samples show why a hit or miss happened.

I also added regression coverage for the intended hot path behavior:

- `full_projection_source_watermark_ignores_source_mtime_when_index_rows_unchanged`
- `full_projection_source_watermark_ignores_session_markdown_churn`
- `full_projection_cache_hit_skips_provider_neutral_rebuild_after_mtime_churn`

Evidence is recorded in `src/tsift/plans/gfullhot-evidence.md`. The three final `backend-eval --full-projection` samples show the expected cold-build then hot-hit shape:

- sample 3: cache miss, `source_graph_build=81160531us`, `projection_rows=20690098us`, `graph_rows=995002`
- sample 4: cache hit, `source_graph_build=0us`, `projection_rows=0us`, `cache_lookup=1473us`, `cache.serde_decode=5413348us`
- sample 5: cache hit, `source_graph_build=0us`, `projection_rows=0us`, `cache_lookup=2578us`, `cache.serde_decode=5691639us`

Committed and pushed the tsift repo change as `551dc57 perf: stabilize full-projection cache key` on `main`. I left unrelated pre-existing dirty files in `src/tsift` untouched.

Verification passed:

- `cargo test full_projection_`
- `make check`
- `cargo build`
- `cargo install --path .`
- GitHub Actions CI run `26501875917`: `clippy + test` passed and live Convex graph acceptance passed

I did not find a separate tmux-test workflow or tmux CI leg in this repo's active GitHub workflows/jobs, so there was no red tmux leg to reproduce locally. The preflight `#next-steps` warning still appears to refer to a post-exchange HTML comment only; I treated it as a non-executable note unless it is moved into the queue or exchange.
### Re: closeout repair — gpt-5

Closeout repair only: the `#gfullhot` response above is already committed, and this write reopens the binary-owned write/commit path after the live IPC `HEAD` marker drift reported by `session-check`.
### Re: #gsqlstage — gpt-5

Completed `#gsqlstage`.

I changed `SqliteGraphStore::replace_projection_with_version` so graph node and edge refresh staging now loads temp tables through bounded multi-row insert chunks instead of one insert execution per graph row. The existing SQL-side delta path still skips unchanged row-hash owner rewrites and only expands materialized property rows for new or changed node/edge owners.

Changed paths in the tsift repo:

- `src/substrate.rs`
- `SPEC.md`
- `plans/gsqlstage-evidence.md`

Evidence is recorded in `plans/gsqlstage-evidence.md`. The three fresh `backend-eval --full-projection` samples from the updated installed binary produced the requested gate metrics:

| Sample | cache hit | graph rows | `sqlite_node_staging` | `sqlite_edge_staging` | `sqlite_delta_write` | `post_write_reads` | `total_duration_micros_per_1k_graph_rows` |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| 1 | 0 | 27,742 | 11,532us | 91,698us | 164,157us | 52,152us | 14,186.648 |
| 2 | 1 | 27,742 | 10,810us | 87,936us | 168,047us | 51,666us | 14,093.829 |
| 3 | 1 | 27,742 | 10,004us | 83,330us | 171,127us | 48,986us | 13,918.030 |

Sample 2 phase details show the new staging path: `bulk stage 4147 graph_nodes rows` and `bulk stage 23595 graph_edges rows` using chunks up to 50 rows.

Committed and pushed the tsift repo change as `6c83b12 perf: bulk stage sqlite graph refresh rows` on `main`. I left the pre-existing unrelated dirty files in `src/tsift` untouched. The superproject still shows `src/tsift` modified because the submodule checkout advanced; I did not create a separate root gitlink commit.

Verification passed:

- `cargo test -q sqlite_projection_refresh_handles_bulk_row_replacement`
- `cargo build`
- `cargo install --path .`
- `make check`
- GitHub Actions CI run `26502654442`: `clippy + test` passed and live Convex graph acceptance passed

The active workflows for this repo are `CI` and `Release`; I did not find a separate tmux-test workflow or tmux CI leg to inspect. The preflight `#next-steps` warning still refers to a post-exchange HTML comment only, so I treated it as a non-executable note.
<!-- agent:boundary:4b0c5ed3 -->
<!-- /agent:exchange -->
###
<!--
What are #next-steps to make the graph faster?
---
What are #next-steps to make the graph + neighborhoods faster + more effective?
-->

<!-- agent:queue auto -->
preset #spec-test-build-install-commit-push
- ~do [#gneighqual]~
- ~do [#gcommqual]~
- ~do [#gfullhot]~
- ~do [#gsqlstage]~
- do [#ghoppolicy]
- do [#gbackendproof]
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog -->
- [ ] [#ghoppolicy] [tag: graph] [tag: performance] Keep max-hop increases opt-in until useful-row evidence passes: compare 64/128/256/512 tiers on real, full-projection, and synthetic deep-chain workloads, require useful deeper rows and flat latency, then consider adaptive caps rather than raising the default blindly.
- [ ] [#gbackendproof] [tag: graph] [tag: performance] Evaluate FalkorDB/Kuzu only as native optional GraphStore adapters: require provider-neutral projection writes/load, freshness/parity, lock behavior, install portability, and faster real/full-projection evidence/conflict/dispatch/path metrics than SQLite before promotion.
<!-- /agent:backlog -->

## Review

<!-- agent:review -->
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
