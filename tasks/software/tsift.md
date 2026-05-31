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

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc65443c45f7b49f5e6ace54e3f6bafe4077ade57924422b5eb3baf4dbd23c-20260531-020618.md`*

Compacted content:
- Archived 6 response topic(s): archived response summary; do [#tsiftmeminit]; do [#tsiftmemhooks]; 3 more
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc6544...
❯ Can tsift now replace claude-mem?

### Re: Can tsift now replace claude-mem? — gpt-5

Not fully yet. It can replace parts of `claude-mem` now, especially first-party `agent-doc` closeout capture and read-only migration staging, but I would not disable `claude-mem` as the only memory source yet.

Current evidence:

- `.tsift/memory.db` is initialized at the workspace root with 3,027 events.
- Of those, 3,000 are imported from `claude-mem` and 27 are first-party tsift events.
- The detected `claude-mem` source still has 32,585 observations, 7,412 session summaries, and 10,599 user prompts available for supported import, so the current import is only a slice of durable history.
- `pending_messages` has 6,561 rows but is intentionally excluded because it is transient queue/tool payload state, not durable replacement memory.
- `agent-doc` closeout capture is wired to `tsift memory capture-agent-doc-closeout` when `.tsift/memory.db` exists, so new closeouts can be captured by tsift-memory in this workspace.
- `graph-db` is materialized and freshness-current, but readiness is blocked by `summary_cache_empty`; semantic retrieval currently returns zero rows until `tsift summarize --extract .` and a graph refresh run.

So the practical answer is: use tsift-memory as the new owner in shadow/transition mode, but keep `claude-mem` as a fallback until three gates pass:

1. Run a full supported import from `claude-mem` with an explicit high enough `--limit` or a no-cap import path.
2. Run `tsift summarize --extract .` and `tsift graph-db --path . refresh --json`, then verify `graph-db related` returns useful memory hits.
3. Switch live retrieval call sites away from direct `claude-mem`/`/mem-search` ownership and verify comparable recall on a few known memories.

Changed paths: none.  
Commands: `tsift memory status . --json`, `sqlite3 .tsift/memory.db ...`, `tsift graph-db --path . --json status`, `tsift graph-db --path . --json related "claude mem replacement" --kind all`, plus targeted source inspection.  
Verification: prompt-only answer; no code or tests changed.  
Confidence: medium-high for current workspace readiness; lower for global replacement until retrieval parity is tested.  
Escalation: none.
❯ What are #next-steps to replace claude-mem?
### Re: What are #next-steps to replace claude-mem? — gpt-5

Top backlog item: `#tsiftmemimportall`.

I added the replacement path to the backlog in this priority order:

1. `#tsiftmemimportall` — complete the full supported `claude-mem` migration into `.tsift/memory.db` and reconcile counts for `observations`, `session_summaries`, and `user_prompts`. Add batching/no-cap import support if the current `--limit` path is not enough.
2. `#tsiftmemsemanticgate` — populate summaries, refresh graph-db, and verify `graph-db related` returns useful tsift-memory hits instead of `summary_cache_empty` / zero semantic rows.
3. `#tsiftmemparityeval` — run the memory retrieval eval fixture against `claude_mem_api`, tsift session-review/context-pack, and `graph_db_related`; require comparable useful-hit rate and fewer zero-output failures before cutover.
4. `#tsiftmemcutover` — switch live retrieval call sites and operator instructions away from direct `claude-mem` / `/mem-search`, with `claude-mem` kept as a fallback during the first pass.
5. `#tsiftmemretire` — retire `claude-mem` only after import, semantic retrieval, and parity eval are green, with a documented rollback path.

I kept `pending_messages` out of the replacement path because tsift already treats it as transient queue/tool payload state rather than durable memory. The replacement contract should be durable memory plus bounded artifact handles, not raw pending tool payload replay.

Changed paths: session document backlog/exchange only.  
Commands: `agent-doc orchestrate --from-exchange --plan`, `agent-doc plan`, `tsift memory status . --json`, `tsift graph-db --path . --json status`, targeted `rg` over tsift-memory/agent-doc surfaces.  
Verification: no code changed; backlog mutation will be checked by `agent-doc finalize` and `session-check`.  
Confidence: high for prioritization; implementation estimates still depend on the full import and retrieval eval results.  
Escalation: none.
<!-- agent:boundary:066a7b6b -->
<!-- /agent:exchange -->
###
<!--
What are #next-steps to improve token efficiency along with graph + neighborhood performance & effectiveness.
---
What are #next-steps to make the graph faster?
---
What are #next-steps to make the graph + neighborhoods faster + more effective?
-->

<!-- agent:queue -->
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog -->
- [ ] [#tsiftmemimportall] Complete a full supported claude-mem migration into .tsift/memory.db with explicit count reconciliation for observations, session_summaries, and user_prompts; add batching/no-cap import support if the current --limit path cannot safely import all rows.
- [ ] [#tsiftmemsemanticgate] Populate the summary cache, refresh graph-db, and verify graph-db related returns useful tsift-memory hits instead of summary_cache_empty / zero semantic rows.
- [ ] [#tsiftmemparityeval] Run the memory retrieval eval fixture against claude_mem_api, tsift session-review/context-pack, and graph_db_related on known memories; require comparable useful-hit rate and fewer zero-output failures before cutover.
- [ ] [#tsiftmemcutover] Replace live memory retrieval call sites and operator instructions that point at direct claude-mem or /mem-search with tsift-memory / graph-db retrieval, while keeping claude-mem as a fallback during the first cutover pass.
- [ ] [#tsiftmemretire] Retire claude-mem only after full import, semantic retrieval, and parity eval are green; keep a documented rollback path until at least one normal session cycle succeeds without direct claude-mem reads.
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
- [ ] [#npm-auth-setup] Configure NPM_TOKEN secret + TSIFT_ENABLE_NPM_PUBLISH=true variable in GitHub repo settings
- [ ] [#npm-publish-verify] After auth configured, tag release and verify publish-opencode-plugin CI job succeeds and opencode-tsift appears on npmjs.org
- [ ] [#full-e2e-verify] In fresh project: opencode plugin opencode-tsift && opencode then /tsift-status. On success mark #e2e-install done.
<!-- /agent:icebox -->

## Completed / Reaped

<!-- agent:done archive=tasks/software/tsift.done.md -->

<!-- completed work archived in tasks/software/tsift.done.md -->
<!-- /agent:done -->
