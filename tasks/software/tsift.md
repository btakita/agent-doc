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

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc65443c45f7b49f5e6ace54e3f6bafe4077ade57924422b5eb3baf4dbd23c-20260604-195054.md`*

Compacted content:
- Archived 5 response topic(s): #edge-index-references; #degree-cache; #community-summary-default; 2 more
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc6544...

### Re: #wal-checkpoint-tuning — glm-5.1

> **Queue prompt:**
>
> do [#wal-checkpoint-tuning]


Added explicit `PRAGMA wal_checkpoint(TRUNCATE)` after each projection refresh and adaptive `wal_autocheckpoint` tuning based on total upserted rows (>10K: 8192 pages, >1K: 4096, else 2048). Prevents WAL growth on repeated refreshes.

Commit: `9e085ae` pushed to main

### Re: #8v37 — glm-5.1 (design analysis)

> **Queue prompt:**
>
> do [#8v37]


**Can tsift provide graph maps for agent visualization?** Yes, through existing commands, but no single "broad map" command exists yet.

**Current capabilities mapping to broad/focused maps:**

| Need | Existing command | Gap |
|------|-----------------|-----|
| Broad map | `communities` + `graph-db status` | No single command combining community structure, top-degree hubs, and edge-kind distribution in token-efficient format |
| Focused map | `graph-db --envelope explain <symbol>` | Already good — callers, callees, community, evidence in one envelope |
| Navigation | `graph-db neighborhood` with `--ultra-terse` | Edge-index references + property sampling reduce tokens ~40-60% |

**Proposed: `tsift graph-db --json map`** — a new command producing a two-tier response:
1. **Overview tier**: community count, top-10 hubs by degree, edge-kind distribution histogram, module-level containment tree. Target: <500 tokens with ultra-terse.
2. **Focus tier** (opt-in): `--focus <symbol>` parameter reuses `explain` envelope for a single deep-dive.

This would reduce tool calls from 3-4 (status → communities → neighborhood → explain) to 1-2 (map → optional focus). The broad map gives enough context for the agent to decide which symbol to focus on.

**Recommendation:** Implement as a composition of existing `communities` + `ranked_neighborhood` + `status` data rather than new SQL queries. The `--ultra-terse` + `--envelope` pipeline already handles token budgets.

Marking done as design analysis. Implementation can be queued as a new backlog item if approved.
<!-- agent:boundary:7d9aa111 -->
<!-- /agent:exchange -->
###
<!--
What are #next-steps to promote SurrealDB to SQLite-equivalent substrate status?
-->

## Queue

<!-- agent:queue preset="#spec-test-build-install-commit-push" -->
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog queue -->
<!-- /agent:backlog -->

## Review

<!-- agent:review -->
- [/] [#astnav-npm] Configure NPM_TOKEN + TSIFT_ENABLE_NPM_PUBLISH secrets, publish opencode-tsift to npm (medium)
- [/] [#surreal-batch-bfs] #surreal-batch-bfs-gated: Batch frontier expansion - gated (low)
- [/] [#surreal-fixture] Record SurrealDB samples in graph-db-performance-history.json fixture (medium)
- [/] [#surrealbulk-close] Close #surrealbulk review item: re-run benchmarks, update SPEC.md benchmark table, mark done
- [/] [#rankdefault] Promote ranked_neighborhood into default graph lookup/context-pack/evidence output only after quality gates pass; compare ranked vs unranked output on real backlog/code queries for useful-hit density, duplicate-name precision, handle coverage, latency, and emitted tokens.
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
- [x] [#lazyperf32-tsift] Cross-posted #lazyperf32 to lazily-rs backlog (lazily-rs project to implement)
- [x] [#lazyperf35-tsift] Cross-posted #lazyperf35 to lazily-rs backlog (lazily-rs project to implement)
- [x] [#lazyperf34-tsift] Cross-posted #lazyperf34 to lazily-rs backlog (lazily-rs project to implement)
- [x] [#lazily-tsift-bench] Cross-posted tsift-pattern benchmarks recommendation to lazily-rs backlog
- [x] [#surreal-rt-pool-warm] Accept external tokio Runtime in open_or_refresh so backend-eval can reuse one Runtime across candidate stores
<!-- completed work archived in tasks/software/tsift.done.md -->
<!-- /agent:done -->
