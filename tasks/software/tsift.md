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

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc65443c45f7b49f5e6ace54e3f6bafe4077ade57924422b5eb3baf4dbd23c-20260603-234544.md`*

Compacted content:
- Archived 8 response topic(s): #surreal-warm; #surreal-delta; #surreal-multiproc; 5 more
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc6544...
<!-- agent:boundary:13f37cb1 -->
<!-- /agent:exchange -->
###
<!--
What are #next-steps to promote SurrealDB to SQLite-equivalent substrate status?
-->

## Queue

<!-- agent:queue preset="#spec-test-build-install-commit-push" -->
- [#surreal-sidecar]
- [#surreal-nodes-by-kind]
- [#surreal-skip-load]
- [#surreal-arc]
- [#surreal-fixture]
- [#surreal-dashmap]
- [#surreal-batch-bfs]
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog -->
- [ ] [#astnav-hooks] Ship tsift-rewrite.sh in-tree + expand PreToolUse equivalent for Codex/OpenCode (medium)
- [ ] [#astnav-npm] Configure NPM_TOKEN + TSIFT_ENABLE_NPM_PUBLISH secrets, publish opencode-tsift to npm (medium)
- [ ] [#astnav-commands] Expand OpenCode command set: /tsift-explain, /tsift-symbol-read, /tsift-graph (low)
- [ ] [#astnav-init-claude] Add tsift init --claude flag for symmetric documentation (low)
- [ ] [#astnav-autoindex-oc] Auto-reindex for OpenCode via plugin freshness check on load (low)
- [ ] [#astnav-harness-docs] Consolidated harness integration guide in README or docs/harness-setup.md (low)
- [ ] [#surreal-sidecar] Persist BTreeMap indexes to sidecar file; skip load_indexes() on cache-hit opens (high)
- [ ] [#surreal-nodes-by-kind] Add by_kind index to SurrealNodeIndexes (10-50x on nodes_by_kind) (high)
- [ ] [#surreal-skip-load] Skip load_indexes() when replace_projection_rows() called immediately after (medium)
- [ ] [#surreal-arc] Switch indexes to Arc<GraphEdge>/Arc<GraphNode> to eliminate clone overhead (medium)
- [ ] [#surreal-fixture] Record SurrealDB samples in graph-db-performance-history.json fixture (medium)
- [ ] [#surreal-dashmap] Replace RwLock<BTreeMap> with HashMap for by_id point lookups (low)
- [ ] [#surreal-batch-bfs] Batch frontier expansion in shortest_path (low)
<!-- /agent:backlog -->

## Review

<!-- agent:review -->
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
- [x] [#surreal-rt-pool-warm] Accept external tokio Runtime in open_or_refresh so backend-eval can reuse one Runtime across candidate stores
<!-- completed work archived in tasks/software/tsift.done.md -->
<!-- /agent:done -->
