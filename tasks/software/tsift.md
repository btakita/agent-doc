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

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc65443c45f7b49f5e6ace54e3f6bafe4077ade57924422b5eb3baf4dbd23c-20260604-051053.md`*

Compacted content:
- Archived 2 response topic(s): tokegate-seed populate token-gate-history.json with baseline samples; tokegate-seed session-check recovery
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc6544...
<!-- agent:boundary:aff03306 -->
<!-- /agent:exchange -->
###
<!--
What are #next-steps to promote SurrealDB to SQLite-equivalent substrate status?
-->

## Queue

<!-- agent:queue preset="#spec-test-build-install-commit-push" auto -->
- ~do [#tokegate-seed]~
- do [#source-body-cap]
- do [#diff-parse-cap]
- do [#evidence-page]
- do [#packet-cache-evict]
- do [#terse-edge-codes]
- do [#sqlite-chunk]
- do [#sqlite-ranked-cte]
- do [#sqlite-conflict-par]
- do [#sqlite-wal-tune]
- do [#sqlite-frontier-temp]
- do [#astnav-npm]
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog queue -->
- [ ] [#source-body-cap] Add body preview token cap to source-read/symbol-read envelopes — long function bodies dominate envelopes with no truncation or expansion command (high)
- [ ] [#diff-parse-cap] Default max_parsed_files to 20-30 in standalone diff-digest — unlimited tree-sitter parsing on large working-tree diffs is wasteful; opt out with --max-parsed-files 0 (medium)
- [ ] [#evidence-page] Cursor-based evidence pagination — single 6k-token cap either floods or cuts aggressively; incremental paging lets agents expand on demand (medium)
- [ ] [#packet-cache-evict] Add cycle packet cache eviction to tsift status --fix — no TTL/size/cleanup policy exists for .tsift/cycle-packet-cache/ (low)
- [ ] [#terse-edge-codes] Map common edge kinds to short codes in ultra-terse — calls->c, defines->d, etc; strip provenance/freshness for further compression (low)
- [ ] [#sqlite-chunk] Increase SQLITE_GRAPH_STAGING_CHUNK_ROWS from 50 to 200-500 and benchmark full-projection refresh — currently ~22s for ~952k rows with ~19k INSERT statements; larger chunks cut statement overhead 4-10x (high)
- [ ] [#sqlite-ranked-cte] Push ranked_neighborhood into recursive CTE with scoring — eliminates N+1 outgoing_edges/node query loop, mirrors existing paged_neighborhood pattern (high)
- [ ] [#sqlite-conflict-par] Parallelize conflict_matrix evidence packet preparation — at 817ms it is the slowest hot-read; per-target traversals are read-only and independent (medium)
- [ ] [#sqlite-wal-tune] Tune WAL_AUTOCHECKPOINT_PAGES from 256 to 1024-4096 and measure refresh overhead reduction — current 256 triggers frequent checkpoints during bulk projection write (medium)
- [ ] [#sqlite-frontier-temp] Pre-allocate temp table for multi-node frontier expansion instead of dynamic WHERE IN string per chunk — amortizes overhead for deep BFS traversals (low)
- [ ] [#astnav-npm] Configure NPM_TOKEN + TSIFT_ENABLE_NPM_PUBLISH secrets, publish opencode-tsift to npm (medium)
<!-- /agent:backlog -->

## Review

<!-- agent:review -->
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
