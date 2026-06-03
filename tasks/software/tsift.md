---
agent_doc_session: tsift-v0.1
resume: 019e85d7-f9ee-79f3-b597-4ed352495bbd
agent: opencode
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
queue_active: false
---

## Status

<!-- agent:status patch=replace -->
<!-- /agent:status -->

## Spec

See [src/tsift/SPEC.md](../../src/tsift/SPEC.md) for the AST-Aware Code RAG spec.

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc65443c45f7b49f5e6ace54e3f6bafe4077ade57924422b5eb3baf4dbd23c-20260603-011149.md`*

Compacted content:
- Archived 11 response topic(s): next steps; #astspansearch; #astspansearch closeout recovery; 8 more
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc6544...
❯ What are #next-steps to save tokens, improve performance, and improve graph lookups?

### Re: next steps for token, performance, and graph lookup work — gpt-5

Top backlog item: `#tokegate`.

I added five active backlog items in priority order. The sequence is: instrument token/perf first, reuse packets inside one agent-doc cycle, promote ranked neighborhoods only after quality gates, then tune SQLite frontier/hop behavior based on measured hotspots, with readiness checks preventing stale or low-signal graph expansion.

The highest leverage work is not swapping databases yet. `src/tsift/SPEC.md` already requires backend-eval and hop-cap promotion gates, and the code already has `next_token_actions`, `GraphEffectivenessReadiness`, `GraphStore::ranked_neighborhood`, and conflict-matrix preparation caching. The gap is turning those surfaces into a visible release gate that proves lower token output and better lookup quality across real sessions.

Commands/proof: ran `agent-doc preflight`, `agent-doc read`, `agent-doc orchestrate --from-exchange --plan`, `agent-doc plan`, `tsift status --fix`, `tsift index --workspace .`, targeted `tsift --envelope search`, and source/spec reads. No code changed, so no build suite was required.

### Re: #tokegate cross-surface token/performance gate — glm-5.1

Implemented `#tokegate` in `tsift_quality::token_gate`.

**What was added:**

- `packages/tsift-quality/src/token_gate.rs` — new module with two gate functions:
  - `evaluate_token_gate` — presence gate: checks every surface has ≥3 samples with non-zero signal on all six metrics
  - `evaluate_token_regression` — regression gate: compares candidate vs baseline medians per metric with configurable regression budget
- Five surfaces: `context_pack`, `session_review_next_context`, `graph_db_evidence`, `conflict_matrix`, `dispatch_trace`
- Six metrics per surface: `prompt_tokens` (lower), `envelope_bytes` (lower), `runtime_micros` (lower), `cache_hit_rate_percent` (higher), `raw_read_avoidance` (higher), `useful_hit_density` (higher)
- `packages/tsift-cli/tests/token_gate.rs` — 9 integration tests
- 8 unit tests inside `token_gate.rs`
- `fixtures/token-gate-history.json` — empty fixture ready for sample recording
- SPEC.md — new section "Cross-Surface Token Gate" documenting surfaces, metrics, and gate contracts
- Root `src/lib.rs` re-exports `tsift_quality::token_gate`

**Verification:**

- `cargo test -p tsift-quality -- token_gate` — 8/8 pass
- `cargo test -p tsift-cli --test token_gate` — 9/9 pass
- `make check` — clean (clippy + build + npm pack dry-run)
- `cargo install --path .` — installed tsift v0.1.62
- Committed and pushed to tsift main: `42738de`

Commands/proof: ran `agent-doc preflight`, `agent-doc read`, `agent-doc plan`, `cargo check`, `cargo test`, `make check`, `cargo install --path .`, `git add` + `git commit` + `git push` in `src/tsift/`.

### Re: #gpackreuse cycle-scoped packet reuse — glm-5.1

> **Queue prompt:**
>
> [#gpackreuse]


Implemented `#gpackreuse` as `tsift_quality::cycle_packet_cache`.

**What was added:**

- `packages/tsift-quality/src/cycle_packet_cache.rs` — new module with disk-backed packet cache:
  - Cache entries under `.tsift/cycle-packet-cache/{evidence,context-pack,impact,conflict-matrix}/`
  - Keys derived from `blake3(version + watermarks + packet-specific params)`
  - `cycle_packet_evidence_key` — deterministic key from `packet_id`
  - `cycle_packet_watermark_key` — triple watermark + extras key
  - Read/write helpers: `cycle_packet_read_cache`, `cycle_packet_write_cache`
  - `CyclePacketCacheEntry` and `CyclePacketCacheHitReport` report types
- `packages/tsift-cli/src/lib.rs` — two integration points:
  - `collect_conflict_matrix_evidence_packets` — checks disk cache before calling `graph_db_evidence_report_from_store`; on hit, returns cached `ConflictMatrixPreparedEvidence` without re-traversing the graph store
  - `build_dispatch_trace_report` — checks disk cache after preparing conflict-matrix inputs; on hit, returns cached `DispatchTraceReport` without rebuilding the conflict-matrix pipeline
  - Added `Clone, Serialize, Deserialize` derives to 15 conflict-matrix types and changed 3 `&'static str` fields to `String` to enable serialization
- `packages/tsift-cli/tests/cycle_packet_cache.rs` — 9 integration tests:
  - Evidence cache key stability and differentiation
  - Watermark key includes all components
  - Disk cache roundtrip for evidence and dispatch-trace
  - Cache miss returns None
  - Packet ID survives roundtrip unchanged
  - Cache directory structure matches spec
  - Cache overwrite replaces stale entry
- 10 unit tests in `cycle_packet_cache.rs`
- `SPEC.md` — new section "Cycle Packet Cache" documenting cache locations, evidence reuse, dispatch-trace reuse, skipped phase proof, and stable packet ID proof

**Verification:**

- `cargo test -p tsift-quality` — 98/98 pass (10 new)
- `cargo test -p tsift-cli` — all pass (9 new integration tests)
- `make check` — clean (clippy + build + npm pack dry-run)
- `cargo install --path .` — installed tsift v0.1.62
- Committed and pushed: `39a4929`

Commands/proof: ran `agent-doc preflight`, `agent-doc read`, `agent-doc plan`, `cargo check`, `cargo test`, `make check`, `cargo install --path .`, `git add` + `git commit` + `git push` in `src/tsift/`.
### Re: code navigation runbook extraction — glm-5.1

Moved the full `<!-- tsift:code-navigation -->` section from inline in AGENTS.md into `runbooks/code-navigation.md`. The Runbooks catalog in AGENTS.md now links to it alongside the other key runbooks, and `code-navigation` appears in the full catalog list.

No code changed; no build suite required. Committed as `174d101`.

> **Queue prompt:**
>
> Code Navigation harness info should be in a Runbook referenced in AGENTS.md or a skill.
Are the Runbook optimizations applied to all projects in agent-loop?

### Re: #rankdefault ranked_neighborhood promotion status — glm-5.1

`ranked_neighborhood` is fully implemented but **held behind a quality gate**. The promotion state:

- **Core BFS** (`tsift-core::GraphStore::ranked_neighborhood`, `lib.rs:887-982`): fully functional, 6 unit tests, three scoring strategies (BreadthFirst, EdgeKindWeighted, DegreeWeighted).
- **Post-hoc scoring** (`tsift-resolution::ranked_neighbors`, `scoring.rs:42-173`): additive `ranked_neighbors[]` field in JSON output alongside default-ordered `nodes[]`.
- **CLI gate** (`tsift-cli::graph_db_neighborhood_ranking_gate`, `lib.rs:12938-12965`): `status: "held_default_order_unchanged"`, `ranked_output_default: false`. The default node order remains `stable_node_id` until the `community_search_quality_metrics` gate passes.

**What `#rankdefault` requires to promote:** three-sample quality comparison on real backlog/code queries measuring useful-hit density, duplicate-name precision, handle coverage, latency, and emitted tokens. The measurement surfaces (`ranked_neighbors_capped`, `NeighborhoodRankingGate`) already exist in the code. The gap is running the actual comparison workload and confirming ranked output meets or exceeds unranked on all six metrics.

No code change needed yet — this is a **measurement and promotion** task, not an implementation task. Recommend scheduling a dedicated measurement cycle against real agent-loop session queries.

### Re: #gfrontierhot SQLite frontier performance triage — glm-5.1

Current SQLite frontier implementation status:

- **Neighborhood** (`tsift-sqlite::paged_neighborhood`, `lib.rs:2290-2469`): single recursive CTE (`WITH RECURSIVE walk`) using `idx_graph_edges_from_kind`, shared node/edge query, cursor-based pagination.
- **Shortest path** (`tsift-sqlite::shortest_path_with_max_hops`, `lib.rs:2481-2599`): iterative BFS with 256-node chunked frontier, prepared single-node statements. Max hops defaults to 64, backend-eval promotion gate measures 128/256/512 tiers.
- **Hop-cap gate** (`tsift-cli::graph_db_backend_eval_hop_cap_promotion_gate`, `lib.rs:16200`): requires 3+ samples on real, full_projection, and synthetic_deep_chain workloads before promoting beyond 64 hops.

**Triage not yet started** — `#gfrontierhot` requires running the three-sample backend-eval workload suite against real agent-loop data. The measurement infrastructure exists. Most agent-loop graphs are small enough that 64 hops never triggers. Recommend running `tsift backend-eval` on the three required workloads and recording results before optimizing anything.

### Re: #greadiness graph-readiness at lookup boundary — glm-5.1

Readiness is **partially implemented** across three boundaries:

**Already gated:** status/doctor reports (`GraphEffectivenessReadiness`, `lib.rs:10235-10242`), related queries (`lib.rs:14276-14305`), context-pack orchestration (`lib.rs:28685-28725`).

**Gap for `#greadiness`:** full suppression of broad low-signal graph expansion is **not wired into every lookup path**. Currently readiness warnings are emitted but graph expansion still proceeds in some paths (e.g., neighborhood queries don't check readiness before expansion). The fix is to thread `fail_closed` through to `GraphDbQuery::Neighborhood` and skip the recursive CTE when readiness is blocked, returning repair commands instead. Small, targeted implementation — recommend after `#gfrontierhot` measurement data is available.

### Re: Runbook optimizations scope across agent-loop projects — glm-5.1

**Not all projects have tsift code-navigation optimizations.** The `tsift:code-navigation` block is **per-project opt-in via `tsift init`**. 10 of ~30 projects have it: tsift, agent-doc, boost-client, corky, equityfundingsource-dev, lazily-rs, lazily-zig, session-share, tmux-router, monsterrodholders-dev. Missing: webmaster, presence, ctx-core-dev, tagpath, and others. The other 14 runbooks in `runbooks/` are repo-wide. To apply to all projects, run `tsift init` in each missing project directory.

### Re: Runbook optimizations — already answered — glm-5.1

The answer was committed in the previous cycle (see "### Re: Runbook optimizations scope across agent-loop projects" above). Striking through the consumed queue items. Backlog items for `#rankdefault`, `#gfrontierhot`, `#greadiness` remain open pending measurement/implementation work.

### Re: tsift init across all agent-doc projects — glm-5.1

> **Queue prompt:**
>
> Run `tsift init` in all projects using tsift in agent-doc


Ran `tsift init` in all 16 projects that have `tsift:code-navigation` in their AGENTS.md. All 16 reported "already present (no changes needed)" — the code-navigation block was already injected in prior cycles.

7 projects had the AGENTS.md block but no `.tsift` directory (agent-doc-js, agent-resume, chromium-bridge, ctx-core-dev, presence, tagpath, webmaster). `tsift init` only manages the AGENTS.md/CLAUDE.md blocks; the `.tsift` index directory is created by `tsift status --fix` or `tsift index` on first use, which happens when those projects are actively indexed.

19 projects (agent-kit, agent-memories, agent-rules, agent-runbooks, briantakita.me-dev, brookebrodack-dev, censible-core, cloudflare, cogov-dev, drmanny-dev, eval-runner, existence-lang, herbaliciousbliss-dev, instruction-files, lazily-py, module-harness, prescience-notes-dev, python-intentions, rappstack-dev, skill-harness) have no AGENTS.md and no CLAUDE.md — they are not currently using agent-doc with tsift. Running `tsift init` there would create new AGENTS.md/CLAUDE.md files with only the code-navigation block, which is outside the scope of this task ("projects using tsift in agent-doc").

No code changes; no build/install/commit needed.

Commands/proof: `tsift init` x16 projects, `tsift init --help`, directory scans for `.tsift`, `AGENTS.md`, `CLAUDE.md`.

### Re: #agent-doc-bug JB Run Agent Doc post-turn stall — glm-5.1

**Root cause identified.** The `dispatch_blocker_reason` function for OpenCode (`harness.rs:324-346`) scans ALL lines of the captured pane output. After a turn completes, the turn's output (bash commands, results, "Thought: 7.6s", "Click to expand") remains visible in the scrollback. These lines are NOT classified as idle chrome, so `has_non_idle_content = true`. The OpenCode idle state has no standalone `>` prompt line, so `has_ready_prompt = false`. Result: false `"opencode active turn"`.

**Contrast with Claude** (`harness.rs:348-364`): the Claude branch checks only the **last 8 lines** for `claude_active_turn_busy()`. Completed turn output in scrollback doesn't cause false positives because it only inspects the bottom of the pane.

**Plan created:** `tasks/agent-doc/plan-opencode-post-turn-false-active.md`

**Recommended fix (Option A):** Replace the OpenCode all-lines scan with a bottom-N-lines busy-cue check (N≈12). A genuine active turn always shows `Working (Ns - esc to interrupt)` at the bottom — the scrollback content is irrelevant. This aligns with the Claude strategy and eliminates the false positive class entirely.

**Bug added** to `agent-doc-bugs2.md` backlog as `#opencode-post-turn-false-active`.

Commands/proof: tmux capture-pane %62, harness.rs source reads (lines 315-346, 258-277, 540-572), route.rs source reads (lines 2499-2551, 8374-8483), plan file created at `tasks/agent-doc/plan-opencode-post-turn-false-active.md`.
<!-- agent:boundary:0ea3b023 -->
<!-- /agent:exchange -->
###
<!--
What are #next-steps to promote SurrealDB to SQLite-equivalent substrate status?
-->

## Queue

<!-- agent:queue preset="#spec-test-build-install-commit-push" auto -->
- /home/brian/work/btakita/agent-loop/src/boost-client/AGENTS.md & /home/brian/work/btakita/agent-loop/src/boost-client/src/monsterrodholders-dev/AGENTS.md has too much in the Code Navigation block. Use the runbooks and optimized session harness to reduce token usage.
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog -->
- [ ] [#rankdefault] Promote ranked_neighborhood into default graph lookup/context-pack/evidence output only after quality gates pass; compare ranked vs unranked output on real backlog/code queries for useful-hit density, duplicate-name precision, handle coverage, latency, and emitted tokens.
- [ ] [#gfrontierhot] Run three-sample performance triage for SQLite neighborhood/path/evidence/related lookups on real, full-projection, high-degree, and deep-chain workloads; optimize recursive frontier expansion or hop caps only where metrics show a hotspot.
- [ ] [#greadiness] Make graph-readiness states actionable at the lookup boundary: if summaries, tagpath handles, graph freshness, or scoped indexes are missing, emit exact repair commands and suppress broad low-signal graph expansion.
<!-- /agent:backlog -->

## Review

<!-- agent:review -->
- [/] [#surrealbulk] Batch SurrealDB projection load: replace per-row upsert loops with bulk/transactional row load plus one-pass secondary-index rebuild; measure refresh/load separately.
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
<!-- /agent:icebox -->

## Completed / Reaped

<!-- agent:done archive=tasks/software/tsift.done.md -->

<!-- completed work archived in tasks/software/tsift.done.md -->
<!-- /agent:done -->
