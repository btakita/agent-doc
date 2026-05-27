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
queue_active: false
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
### Re: #ghoppolicy — gpt-5

Completed `#ghoppolicy`.

I kept the user-facing graph path default at 64 hops and tightened the promotion policy proof instead of raising the cap. Backend-eval already reports 64/128/256/512 `path_max_hops` duration and row metrics for the required SQLite workloads; this cycle locked that contract in the graph DB conformance test so partial tier evidence cannot be mistaken for promotion evidence.

Changed paths in the tsift repo:

- `SPEC.md`
- `tests/graph_db_conformance.rs`
- `plans/ghoppolicy-evidence.md`

Evidence is recorded in `plans/ghoppolicy-evidence.md`. The three fresh full-projection samples show useful deep-chain rows at higher tiers, but not flat latency:

| Workload | Hop cap | Median duration | Median rows |
| --- | ---: | ---: | ---: |
| real | 64 | 85us | 2 |
| real | 128 | 38us | 2 |
| real | 256 | 33us | 2 |
| real | 512 | 31us | 2 |
| full_projection | 64 | 71us | 2 |
| full_projection | 128 | 35us | 2 |
| full_projection | 256 | 31us | 2 |
| full_projection | 512 | 30us | 2 |
| synthetic_deep_chain | 64 | 89us | 65 |
| synthetic_deep_chain | 128 | 139us | 129 |
| synthetic_deep_chain | 256 | 267us | 257 |
| synthetic_deep_chain | 512 | 530us | 513 |

That means the policy holds: deep-chain rows prove higher caps are useful, but the 512-hop median is 530us versus the 64-hop median of 89us, so the 10% regression budget blocks promotion and keeps higher caps opt-in.

Committed and pushed the tsift repo change as `00f9e54 test: lock hop cap promotion metrics` on `main`. I left the pre-existing unrelated dirty files in `src/tsift` untouched. The superproject still shows `src/tsift` modified because the submodule checkout advanced; I did not create a separate root gitlink commit.

Verification passed:

- `cargo test -q graph_db_backend_eval_benchmarks_candidate_stores_against_sqlite`
- `cargo test -q hop_cap_gate`
- `cargo build`
- `make check`
- `cargo install --path .`
- GitHub Actions CI run `26503222930`: `clippy + test` passed and live Convex graph acceptance passed

The active workflows for this repo are `CI` and `Release`; I did not find a separate tmux-test workflow or tmux CI leg to inspect. The preflight `#next-steps` warning still refers to a post-exchange HTML comment only, so I treated it as a non-executable note.
### Re: JB File Cache Conflict repair — gpt-5

Repaired the `tasks/software/tsift.md` corruption from the JetBrains File Cache Conflict cancel.

I removed the stale replayed exchange tail, restored the session document to a single balanced `agent:queue` component, and kept the queue at the current committed state with only the backend proof item still open. `agent-doc session-check` passes for the document.

For `#agent-doc-bug`, the existing backlog item `#0ep7` already tracks this class through `tasks/agent-doc/plan-jb-cache-conflict-cancel-uncommitted-response.md`. I updated that plan with this live `tsift.md` duplicate-response and duplicate-queue-tag incident and committed it as `1d34183c7`.
❯ Can tsift be made into a knowledge layer for realtime avatars and for agents?
❯ tsift start off to navigate code, but it can also navigate markdown.
❯ Should we have a separate project or should tsift support the db/knowledge retrieval for realtime voice avatars?
### Re: tsift as an avatar/agent knowledge layer — gpt-5

Yes, tsift can become the knowledge layer for realtime avatars and agents, but the boundary matters.

The right split is: keep tsift as the substrate/index/retrieval engine, and put avatar runtime behavior in a separate integration layer. Tsift already has the pieces that should stay shared: provider-neutral `GraphStore` records, provenance/freshness, markdown/session projections, semantic rows, evidence packets, neighborhoods, paths, Convex sync, and optional backend evaluation. Those are useful for both coding agents and realtime avatars.

What should not live directly in tsift is the product/runtime loop: LiveKit session state, voice interruption handling, persona policy, user-facing memory consent, per-user privacy rules, and model-specific response orchestration. Those should be in a small avatar/agent service or adapter that calls tsift for retrieval.

So I would not create a separate knowledge database project yet. I would make tsift support the generic retrieval needs, then create a separate application adapter only for realtime voice specifics. Forking the knowledge substrate now would create two graph schemas, two freshness models, and two retrieval stacks before we know the actual latency and memory requirements.

The missing proof is not whether tsift can store markdown. It can. The missing proof is whether it can serve the realtime shape well:

- conversation transcript and memory adapters, not only code/session docs
- topic/entity/event/time edges for non-code knowledge
- low-latency read API, likely library or daemon, not only CLI process startup
- incremental freshness for live conversations and edited markdown
- permission/delete semantics for personal memory
- retrieval evals for avatar turns: answer relevance, recency, source grounding, and p95 latency

I added backlog item `#gavatar` to make that an explicit evaluation slice. The decision gate should be: if tsift can satisfy the avatar retrieval API with stable latency and clean privacy/freshness semantics, keep it as the common knowledge layer. If the realtime service needs domain-specific session state or persona policy, build that as a separate app layer on top, not as a competing database.
<!-- agent:boundary:79a9d952 -->
<!-- /agent:exchange -->
###
<!--
What are #next-steps to make the graph faster?
---
What are #next-steps to make the graph + neighborhoods faster + more effective?
-->

<!-- agent:queue -->
preset #spec-test-build-install-commit-push
- ~do [#gneighqual]~
- ~do [#gcommqual]~
- ~do [#gfullhot]~
- ~do [#gsqlstage]~
- ~do [#ghoppolicy]~
- do [#gbackendproof]
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog -->
- [ ] [#gavatar] [tag: knowledge-layer] [tag: realtime] Evaluate tsift as a realtime avatar/agent knowledge layer: define GraphStore adapters for markdown/conversation memory, a low-latency retrieval API, freshness/privacy rules, and benchmark fixtures before deciding whether to split an application-specific service.
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
