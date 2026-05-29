---
agent_doc_session: tsift-v0.1
agent: claude
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

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc65443c45f7b49f5e6ace54e3f6bafe4077ade57924422b5eb3baf4dbd23c-20260529-014716.md`*

Compacted content:
- Archived 2 response topic(s): do [#cli-extract-output]; do [#cli-extract-output-envelope]
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc6544...
<!-- agent:boundary:3f3e4d80 -->
<!-- /agent:exchange -->
###
<!--
What are #next-steps to make the graph faster?
---
What are #next-steps to make the graph + neighborhoods faster + more effective?
-->

<!-- agent:queue auto -->
preset #spec-test-build-install-commit-push
- do [#cli-extract-cli-types]
- do [#cli-cluster-index-search]
- do [#cli-cluster-graph]
- do [#cli-extract-tagpath-helpers]
- do [#cli-cluster-quality]
- do [#cli-cluster-summarize]
- do [#cli-cluster-infra]
- do [#cli-cluster-digests]
- do [#npm-auth-setup]
- do [#npm-publish-verify]
- do [#full-e2e-verify]
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog -->
- [ ] [#cli-extract-cli-types] Phase 1b — lift clap CLI types (Cli, Commands, GraphDbQuery, output format enums) out of lib.rs into packages/tsift-cli/src/cli.rs. Pure mechanical lift; no logic change.
- [ ] [#cli-cluster-index-search] Phase 2 — move cmd_index + cmd_search bodies into packages/tsift-cli/src/commands/index_search.rs (re-exported from lib.rs). Smallest cluster, validates the per-command pattern before larger ones.
- [ ] [#cli-cluster-graph] Phase 2 — move cmd_graph + cmd_communities + cmd_path + cmd_explain + cmd_traverse bodies into packages/tsift-cli/src/commands/graph.rs.
- [ ] [#cli-extract-tagpath-helpers] Phase 1a-iii — extract TagpathSearchOpts + TagpathAnnotationDiagnostic + CommunityMemberAmbiguityDiagnostic + annotate_hits_with_tagpath + annotate_stored_symbols_with_tagpath + annotate_stored_edges_with_tagpath + annotate_communities_with_tagpath + annotate_path_nodes_with_tagpath (~700 LOC) into packages/tsift-cli/src/output/tagpath.rs. Sits below the graph/community cmd cluster but moves cleanly because the annotate functions are already pub.
- [ ] [#cli-cluster-quality] Phase 2 — move cmd_audit + cmd_audit_tagpath + cmd_lint + cmd_perf_gate bodies into packages/tsift-cli/src/commands/quality.rs.
- [ ] [#cli-cluster-summarize] Phase 2 — move cmd_summarize body (and helpers) into packages/tsift-cli/src/commands/summarize.rs.
- [ ] [#cli-cluster-infra] Phase 2 — move cmd_init + cmd_status + cmd_locks + cmd_sql + cmd_route + cmd_rewrite + cmd_convex_sync + cmd_graph_db_* + cmd_edit bodies into packages/tsift-cli/src/commands/infra.rs.
- [ ] [#cli-cluster-digests] Phase 2 — move cmd_diff_digest + cmd_log_digest + cmd_test_digest + cmd_metric_digest + cmd_session_digest + cmd_session_cost + cmd_session_review + cmd_context_pack bodies into packages/tsift-cli/src/commands/digests.rs. Depends on tsift-digest and tsift-session landing first.
- [ ] [#npm-auth-setup] Configure NPM_TOKEN secret + TSIFT_ENABLE_NPM_PUBLISH=true variable in GitHub repo settings
- [ ] [#npm-publish-verify] After auth configured, tag release and verify publish-opencode-plugin CI job succeeds and opencode-tsift appears on npmjs.org
- [ ] [#full-e2e-verify] In fresh project: opencode plugin opencode-tsift && opencode then /tsift-status. On success mark #e2e-install done.
<!-- /agent:backlog -->

## Review

<!-- agent:review -->
- [/] [#split-tsift-cli-subcommands] [recommended] tsift-cli is ~39k LOC in a single lib.rs — propose per-command module split (cmd_search, cmd_index, cmd_graph, cmd_explain, etc.) once root tsift is drained, to make command surface independently testable
- [/] [#demote-root-tsift] After all module splits land, demote root tsift crate to a thin re-export shim (or remove lib.rs entirely) — leave only the workspace root + binary metadata; verify external consumers (cargo install tsift) keep working
- [/] [#split-tsift-search] Extract sift + summarize + impact + tagpath_adapter (~3.2k LOC) into tsift-search crate — depends on tsift-index + tsift-graph; covers search ranking, AST-aware summaries, impact analysis, and tagpath annotation surface
- [/] [#split-tsift-digest] Extract diff_digest + log_digest + metric_digest + test_digest (~3.7k LOC) into tsift-digest crate — high-cohesion emitter group with shared serialization, schema versioning, and pruning patterns; minimal cross-deps
- [/] [#split-sim-world] Extract sim_world (~390 LOC) into tsift-sim-world crate — smallest, most isolated module; low-risk pattern validation for the remaining splits; keeps sim/test helpers out of the prod-path tree
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
