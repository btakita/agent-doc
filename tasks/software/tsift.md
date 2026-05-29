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

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc65443c45f7b49f5e6ace54e3f6bafe4077ade57924422b5eb3baf4dbd23c-20260529-055921.md`*

Compacted content:
- Archived 3 response topic(s): do [#cli-extract-tagpath-helpers]; do [#cli-cluster-quality]; do [#cli-cluster-summarize]
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc6544...

### Re: do [#cli-cluster-infra] — opus-4-8

Phase 2 cluster #5 done — the largest and most entangled cluster.

#### What moved (15 functions, ~1371 LOC) → `packages/tsift-cli/src/commands/infra.rs`

- `cmd_route` — model-tier classification
- `cmd_edit` — atomic batch file edits
- `cmd_convex_sync` — Convex nodes/edges sync planning + transport
- `cmd_graph_db_status` / `cmd_graph_db_refresh` / `cmd_graph_db_doctor` / `cmd_graph_db_drift` / `cmd_graph_db_compact` / `cmd_graph_db_backend_eval` / `cmd_graph_db` — the full graph-db operator surface (`backend_eval` alone is ~423 LOC)
- `cmd_status` / `cmd_locks` — session health + lock diagnostics
- `cmd_init` — project setup
- `cmd_sql` — SQLite introspection
- `cmd_rewrite` — shell→tsift rewrite

lib.rs re-imports the 9 top-level entry points; the six `cmd_graph_db_*` sub-handlers are called internally from `cmd_graph_db`, so only `cmd_graph_db` crosses the module boundary.

#### Approach: compiler-driven promotion

This cluster is far more coupled than the prior ones (51 distinct method calls, 15 private types, several with impl blocks). Rather than hand-trace every dependency, I moved the bodies and let `cargo build` enumerate every private symbol needing `pub(crate)` — iterating until clean. Promoted:

- **~59 helper fns** (graph-db operator/backend-eval/convex helpers, edit-plan, status-fix, sql helpers)
- **15 private types** — `ConvexHttpTransport`, `ConvexSyncOptions`, `EditBatch`/`EditResult`/`EditStatus`, the `GraphDbBackendEval*` family, `GraphDb{Compaction,Doctor,Drift,Evidence}*`, `GraphDbExperimentalBackend`, `GraphDbRefreshSummary`
- **5 `GRAPH_*` consts**

Helpers and the `GraphDbExperimentalBackend` business-logic impl **stay in lib.rs** — they're shared with the still-resident conflict-matrix / dispatch-trace / dependency-dag evidence commands. Only the cmd bodies moved.

#### One subtlety worth recording

`cargo fix` (run to clean unused imports) stripped `GraphDbBackend` + `TraverseFormat` from lib.rs's non-test cli import — correct for the non-test build, but the `#[cfg(test)]` module still references those enum variants. `cargo fix --lib` doesn't analyze test cfg, so the lib built clean while `cargo test -p tsift-cli` broke with E0433. Restored them as a `#[cfg(test)]`-only import. Caught because I run `cargo test -p tsift-cli` explicitly — note that `make check`'s `cargo test` only exercises the root `tsift` package (56 tests), not the tsift-cli package's 479, though clippy `--all-targets` does compile them.

#### Verification

- `cargo build -p tsift-cli` — clean (only the 9 pre-existing non-test `dead_code` warnings, which are live under test cfg)
- `cargo test -p tsift-cli` — **479 passed across 7 suites**
- `make check` — green (clippy `--all-targets -D warnings`, cargo test, opencode npm dry-run)
- `cargo install --path packages/tsift-cli` + `cargo build --release -p tsift-cli` — both binaries refreshed; `tsift --version` → `0.1.62`

#### Commits

- tsift @ `5834ac5` — `cli: Phase 2 — extract infra command cluster into commands/infra.rs` (pushed)
- agent-loop @ `0e6bf04` — submodule pointer bump (pushed)

VERSIONS.md `Unreleased` updated. lib.rs now ~34.2k LOC — down from ~39.2k at Phase 1a-i kickoff (~5k LOC / 13% drained across the split).

#### Top of backlog

`[#cli-cluster-digests]` — Phase 2 (final cluster): move `cmd_diff_digest`, `cmd_log_digest`, `cmd_test_digest`, `cmd_metric_digest`, `cmd_session_digest`, `cmd_session_cost`, `cmd_session_review`, `cmd_context_pack` bodies into `packages/tsift-cli/src/commands/digests.rs`. The backlog notes this depends on `tsift-digest` and `tsift-session` crates landing first — both already extracted (per VERSIONS `#split-tsift-session` / digest history), so the cmd bodies should lift cleanly.
### Re: do [#cli-cluster-digests] — opus-4-8

Phase 2 cluster #6 done — **the per-command-cluster split is complete.**

#### What moved (9 functions, ~1347 LOC) → `packages/tsift-cli/src/commands/digests.rs`

- `cmd_diff_digest` — code-aware worktree/staged/revision diff digest
- `cmd_test_digest` / `cmd_log_digest` — grouped test-failure / verbose-log digests
- `cmd_context_pack` — composes session-review + diff + test/log digests into one handoff pack
- `cmd_metric_digest` — repeated metric-run deltas + news tables
- `cmd_session_digest` / `cmd_session_cost` — transcript prompt/command/cost digests
- `cmd_session_review` (test-only wrapper) + `cmd_session_review_with_budget` (production engine) — cross-harness aggregate review

lib.rs re-imports the production entry points; `cmd_session_review_with_budget` is the real engine (the thin `#[allow(dead_code)]` `cmd_session_review` wrapper stays in digests.rs, not re-imported — same pattern as `cmd_search`/`cmd_explain`).

The backlog noted this cluster "depends on `tsift-digest` and `tsift-session` landing first" — both already extracted in prior cycles, so the cmd bodies lifted cleanly (they call `tsift::diff_digest::*`, `tsift::metric_digest::*`, `tsift::session_*::*` as fully-qualified library paths).

#### Visibility changes (mechanical)

Promoted 16 helper fns to `pub(crate)` (diff-digest label helpers, metric-digest gate/trend labels, `render_{test,log}_digest_from_input`, the session-review/context-pack budget-report builders + human printers, `format_compact_count`) plus the `MetricDigestOptions` struct.

**Trap avoided:** a naive symbol scan flagged `report` and `edges` as private items to promote — they're actually JavaScript `const` declarations inside embedded HTML-viz string literals (graph-data / trace-data). Blindly running the `const` promotion would have corrupted those string contents. Excluded them.

#### Verification

- `cargo build -p tsift-cli` — clean, no new warnings
- `cargo test -p tsift-cli` — **479 passed across 7 suites**
- `make check` — green
- `cargo install --path packages/tsift-cli` + `cargo build --release -p tsift-cli` — both binaries refreshed; `tsift --version` → `0.1.62`

#### Commits

- tsift @ `9175c04` — `cli: Phase 2 — extract digests command cluster into commands/digests.rs` (pushed)
- agent-loop @ `794fa65` — submodule pointer bump (pushed)

#### Split complete — final shape

Every `cmd_*` body now lives in a focused module:

| Module | LOC |
|--------|-----|
| `commands/infra.rs` | 1491 |
| `commands/digests.rs` | 1381 |
| `commands/graph.rs` | 969 |
| `commands/index_search.rs` | 824 |
| `commands/quality.rs` | 424 |
| `commands/summarize.rs` | 293 |
| `output/tagpath.rs` | 321 |
| **total extracted** | **~5.7k LOC** |

`lib.rs` is now **32,828 LOC** — down from **39,246** at the Phase 1a-i kickoff (**~6.4k LOC / 16% drained**). What remains in lib.rs is the shared helper/type substrate (graph-db evidence helpers, backend-eval business logic, the still-resident `cmd_conflict_matrix` / `cmd_dispatch_trace` / `cmd_dependency_dag` / `cmd_semantic_related` / `cmd_source_read` / `cmd_workflow` / `cmd_digest_runner` commands, plus the large test module).

#### Backlog status

All six CLI-split clusters plus `#cli-extract-{cli-types,tagpath-helpers}` are done. The remaining open backlog items (`#npm-auth-setup`, `#npm-publish-verify`, `#full-e2e-verify`) are the OpenCode-plugin npm publish track — a separate workstream gated on GitHub repo-settings access (NPM_TOKEN secret + `TSIFT_ENABLE_NPM_PUBLISH` variable), which you pruned from the active queue this cycle. They stay open; no remaining queued work.
<!-- agent:boundary:15ac0d09 -->
<!-- /agent:exchange -->
###
<!--
What are #next-steps to make the graph faster?
---
What are #next-steps to make the graph + neighborhoods faster + more effective?
-->

<!-- agent:queue -->
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog -->
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
