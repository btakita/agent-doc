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

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc65443c45f7b49f5e6ace54e3f6bafe4077ade57924422b5eb3baf4dbd23c-20260601-222220.md`*

Compacted content:
- Archived 5 response topic(s): #surreal-backend-eval; #surreal-parity-suite; #surreal-native-adapter; 2 more
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc6544...
❯ Is surrealdb at parity with sqlite?

### Re: SurrealDB parity — gpt-5

Short answer: yes for the current backend-eval parity surface, no for production replacement parity with SQLite.

The conformance path asserts SurrealDB's backend-eval report has `parity.matches_sqlite == true` and no diagnostics for each dataset, then compares SurrealDB row counts against SQLite for the parity operations. The spec says the SurrealDB prototype is compared against SQLite across status/count rows, edge lookup, edge-property scan, incident scan, neighborhood, related retrieval, path tiers, evidence, conflict-matrix, and dispatch-trace.

But that is still backend-eval/prototype parity. It is not promoted to SQLite-equivalent substrate status. The promotion gate keeps SurrealDB behind backend-eval until a real optional adapter proves provider-neutral projection writes/load, full `GraphStore` parity, writer/read-only lock behavior, default build/install portability, full-projection wins, and faster-than-SQLite results across the real, full-projection, high-degree, and deep-chain workloads. The code also labels the default SurrealDB path as a read snapshot unless the `backend-surrealdb` feature is enabled.

So I would treat it as: `SurrealDB is parity-checked and currently passes the measured read/query parity suite; SQLite remains the stable baseline and default until SurrealDB passes the production adapter and performance gates.`

Evidence checked: `SPEC.md`, `packages/tsift-cli/src/lib.rs`, and `packages/tsift-cli/tests/graph_db_conformance.rs`. No repo files changed.
❯ What is the performance of SurrealDB compared to SQLite?

### Re: SurrealDB performance — gpt-5

I ran the local backend-eval comparison with `--candidate surrealdb`, including the opt-in `--full-projection` gate. The result is: SurrealDB still passes parity, but it does not beat SQLite overall in the promotion-relevant workloads.

Latest `--full-projection` totals:

| dataset | SQLite | SurrealDB | result |
| --- | ---: | ---: | --- |
| real | 17.6 ms | 375.2 ms | SurrealDB ~21x slower |
| synthetic_high_degree | 17.6 ms | 9.2 ms | SurrealDB ~1.9x faster |
| synthetic_deep_chain | 15.7 ms | 6.4 ms | SurrealDB ~2.5x faster |
| full_projection | 38.4 s | 50.7 s | SurrealDB ~1.3x slower |

The important detail is the operation mix. SurrealDB wins some synthetic totals because its prototype load/refresh path is cheaper there, and it is faster on some deep-chain path/evidence-style operations. But it loses badly on the operations that matter for replacing SQLite on the real/full graph: full-projection `edge_lookup` was 4.30 s vs SQLite 13.2 ms, `edge_property_scan` was 2.00 s vs 27.7 ms, `incident_edges` was 3.62 s vs 0.39 ms, `related` was 24.5 s vs 19.3 ms, and path probes were seconds vs tens of microseconds.

So the current conclusion is: `SurrealDB is parity-correct but not performance-competitive enough to promote.` The backend-eval promotion decision remains `hold`, with the explicit gate still requiring a real optional adapter, provider-neutral projection writes/load, SQLite parity on real and full-projection datasets, matching-or-better lock behavior, install portability, and faster-than-SQLite results across the required workloads.

Commands run: `tsift status`, `agent-doc preflight`, `agent-doc plan`, `tsift graph-db --path /home/brian/work/btakita/agent-loop/tasks/software/tsift.md --json backend-eval --candidate surrealdb --target gval`, and the same backend-eval command with `--full-projection`. No repo code files were changed.
<!-- agent:boundary:51a5db8b -->
<!-- /agent:exchange -->
###
<!--
What are #next-steps to promote SurrealDB to SQLite-equivalent substrate status?
-->

## Queue

<!-- agent:queue -->
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog -->
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
<!-- /agent:icebox -->

## Completed / Reaped

<!-- agent:done archive=tasks/software/tsift.done.md -->

<!-- completed work archived in tasks/software/tsift.done.md -->
<!-- /agent:done -->
