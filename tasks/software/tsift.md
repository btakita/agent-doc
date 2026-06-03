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
<!-- agent:boundary:0199a745 -->
<!-- /agent:exchange -->
###
<!--
What are #next-steps to promote SurrealDB to SQLite-equivalent substrate status?
-->

## Queue

<!-- agent:queue preset="#spec-test-build-install-commit-push" auto -->
- ~[#gpackreuse]~
- Code Navigation harness info should be in a Runbook referenced in AGENTS.md or a skill.
- [#rankdefault]
- [#gfrontierhot]
- [#greadiness]
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
