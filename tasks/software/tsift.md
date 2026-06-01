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

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc65443c45f7b49f5e6ace54e3f6bafe4077ade57924422b5eb3baf4dbd23c-20260601-081227.md`*

Compacted content:
- Archived 11 response topic(s): lazily-rs for tsift caching and lazy-load graphs; #next-steps to integrate lazily-rs into tsift; status --json auto-fix stale index; 8 more
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc6544...
❯ Please review the usage of lazily-rs. Are the caches properly invalidated? #next-steps

### Re: lazily-rs cache invalidation — gpt-5

No blocking cache-invalidation bug found in the current lazily-rs usage.

Findings:
- `tsift-index` inspect sharing is guard-scoped and thread-local. The key is `(db_path, root, prune)`, the outermost `InspectScopeGuard` drops the whole context, errors clear the slot, and writer paths bump the epoch after index refreshes in `ensure_query_index_current` and `prepare_agent_doc_index_gate`. That means the cached read-only inspection is reused only inside the intended trusted scope and is invalidated after in-process index mutation.
- `tsift-status` uses the same epoch pattern for one `tsift status` command. `cmd_status` invalidates the status cache after missing-scope autoindex and after status fixes before recomputing the report, so the auto-fix path does not report stale pre-fix inspection results.
- `tsift-summarize` keys summary snapshots by normalized file path plus a content-hash cell. Hash changes cause a slot recompute; successful extraction calls `replace_file` and then `invalidate_file`; deleted summaries are also invalidated. This is correct for the command-local cache under the summary write lock.
- `tsift-graph` call-edge caching keys by `(file, content_hash)` and also depends on the file mtime cell. A new content hash creates a new slot; mtime-only churn recomputes but should return the same edges. Since the captured symbols/call-sites come from the same file bytes, this is safe for the current indexer path.
- The `tsift-core` lazily usage in neighborhood and ranked-neighborhood traversal is not a persistent cache. It is a per-call lazy layer chain, so there is no cross-call invalidation surface.
- Backend-eval/full-projection cache invalidation is driven by source watermarks, not lazily slots. The watermark logic intentionally ignores generated/runtime/mtime-only churn and includes semantic index rows plus summary rows; targeted tests cover both stability and semantic invalidation.

Residual risk: the safe assumptions are mostly implicit. `StatusCheckCache`, `SummaryCache`, and `ResolveEdgesCache` would be easier to audit if the cache key/lifetime/invalidation contract lived next to the spec rather than only in tests and local comments. I added that as the top backlog follow-up.

Top backlog item: `#lazily-cache-contracts`.

Changed paths:
- `tasks/software/tsift.md` only (session response and backlog item)

Commands:
- `tsift status`
- `tsift status --fix`
- `agent-doc --version`
- `agent-doc preflight /home/brian/work/btakita/agent-loop/tasks/software/tsift.md`
- `agent-doc read /home/brian/work/btakita/agent-loop/tasks/software/tsift.md`
- `agent-doc plan /home/brian/work/btakita/agent-loop/tasks/software/tsift.md`
- `tsift --envelope search "lazily" --budget normal`
- `tsift --envelope session-review /home/brian/work/btakita/agent-loop/tasks/software/tsift.md --next-context --budget normal`
- `tsift rewrite --run 'cargo test -p tsift-index inspect_scope_lazily_reuses_until_epoch_invalidation'`
- `tsift rewrite --run 'cargo test -p tsift-status status_cache_reuses_index_inspection_until_invalidated'`
- `tsift rewrite --run 'cargo test -p tsift-summarize summary_cache'`
- `tsift rewrite --run 'cargo test -p tsift-graph resolve_edges_cache_reuses_slots_until_mtime_or_hash_changes'`
- `tsift rewrite --run 'cargo test -p tsift-cli full_projection_source_watermark'`
- `tsift rewrite --run 'cargo test -p tsift-cli traversal_source_watermark'`

Verification:
- All focused cache-invalidation regression tests above passed.
- `src/tsift` worktree remained clean after the review and test runs.

Confidence: medium-high. The review covered all current `lazily` call sites and the adjacent backend-eval source-watermark caches, but I did not run the full workspace suite because no source files were changed.

Escalation: none.
<!-- agent:boundary:0a2b56d6 -->
<!-- /agent:exchange -->

## Queue

<!-- agent:queue -->
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog -->
- [ ] [#lazily-cache-contracts] [recommended] Document each lazily-rs cache key, lifetime, invalidation trigger, and matching regression test in src/tsift/SPEC.md so future cache additions have an explicit audit checklist.
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
