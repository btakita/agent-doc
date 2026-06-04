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

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc65443c45f7b49f5e6ace54e3f6bafe4077ade57924422b5eb3baf4dbd23c-20260604-180831.md`*

Compacted content:
- Archived 2 response topic(s): sqlite graph performance next-steps; token efficiency next-steps
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc6544...

### Re: #sqlite-doc-sync-safety — glm-5.1 (HEAD)

Added rustdoc on both `SqliteGraphStore` and `SqliteReadOnlyConnection` documenting the single-active-call-per-connection invariant for temp-table methods. Updated SPEC.md with the same invariant.

- Build: clean. Install: v0.1.62. Tests: 13/13 passed.
- Commit: c5ec8d9 pushed to main.

### Re: queue reactivation — glm-5.1 (HEAD)

Confirmed `#sqlite-doc-sync-safety` done (already in done archive). Queue was halted because the head item was modified between cycles; reactivated `queue_active: true`. Next queue item: `do [#sqlite-reentrant-guard]`.
<!-- agent:boundary:56c4eb8a -->
<!-- /agent:exchange -->
###
<!--
What are #next-steps to promote SurrealDB to SQLite-equivalent substrate status?
-->

## Queue

<!-- agent:queue preset="#spec-test-build-install-commit-push" -->
- ~do [#frontier-unique-temp]~
- ~do [#temp-table-audit]~
- ~do [#sqlite-doc-sync-safety]~
- do [#sqlite-reentrant-guard]
- do [#refresh-skip-unchanged-staging]
- do [#terse-uniform]
- do [#edge-index-references]
- do [#degree-cache]
- do [#community-summary-default]
- do [#property-sampling]
- do [#edges-between-batch-optimization]
- do [#wal-checkpoint-tuning]
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog queue -->
- [ ] [#refresh-skip-unchanged-staging] Pre-staging row-hash comparison to skip unchanged rows before temp INSERT in projection refresh — target <5s for <1% change rates (high)
- [ ] [#terse-uniform] Audit and enforce TerseGraphNode/TerseGraphEdge on all agent-facing envelope output paths — est 15-25% token reduction (high)
- [ ] [#edge-index-references] Compress edge output with positional node index references instead of full ID strings — est 30-50% edge token reduction (medium)
- [ ] [#degree-cache] Pre-compute and cache degree counts for DegreeWeighted ranked_neighborhood scoring (medium)
- [ ] [#community-summary-default] Community-aware truncation summary in neighborhood output when max_nodes prunes results (medium)
- [ ] [#property-sampling] Add property_mode (full|sample|omit) to RankedNeighborhoodOptions for kind-redundant property maps (low)
- [ ] [#edges-between-batch-optimization] Use IN-clause expansion or session-scoped temp table for small edges_between_nodes ID sets (low)
- [ ] [#wal-checkpoint-tuning] Add explicit PRAGMA wal_checkpoint(TRUNCATE) after refresh; tune autocheckpoint by projection size (low)
- [ ] [#sqlite-reentrant-guard] [recommended] Add runtime Cell<bool> guard that panics on re-entrant temp-table calls on same connection (low)
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
