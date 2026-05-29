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
### Re: do [#cli-extract-cli-types] — opus-4-7

Phase 1b done. Mechanical lift of the clap CLI surface out of `packages/tsift-cli/src/lib.rs` (38,260 LOC after) into a new `packages/tsift-cli/src/cli.rs` (999 LOC):

- `Cli` struct + `Commands` enum (~770 LOC across 40 subcommands)
- `GraphDbQuery` subcommand enum (~150 LOC across 15 variants)
- `TraverseFormat`, `DispatchTraceFormat`, `SemanticRelatedKind`, `GraphDbBackend` `ValueEnum`s

Lib.rs now does `mod cli; use cli::{...}` and the `use clap::...` line drops to just `clap::Parser` (needed for `Cli::parse()`). Visibility on every field/variant promoted to `pub` so the ~130 `Commands::*` / `GraphDbQuery::*` dispatch arms, two `Cli::parse_from` test helpers, and the main `Cli::parse()` entry all keep compiling unchanged.

Intentionally left in `lib.rs`: `GraphDbExperimentalBackend` + `GraphDbBackendPromotionGate` + their `impl` block (business logic for `backend-eval`, not parser surface).

#### Verification

- `cargo build -p tsift-cli` — clean, no new warnings (the two pre-existing `GraphFreshness` / `DEFAULT_BUDGET_*` unused-import warnings are unchanged; verified via `git stash` round-trip).
- `cargo test -p tsift-cli --lib` — 298 passed
- `cargo test -p tsift-cli --test exit_code` — 129 passed
- `cargo install --path packages/tsift-cli` — replaces `~/.cargo/bin/tsift` for tmux-spawned sessions
- `cargo build --release -p tsift-cli` — refreshes `target/release/tsift` for the `.bin/...` symlink path
- `tsift --version` → `tsift 0.1.62`, `tsift index --help` renders all flags correctly

#### make check status

`make check` is red, but **all 7 failures are pre-existing** — confirmed by stashing my changes and re-running:

- `unused import: GraphFreshness` (in tsift-cli lib.rs)
- `tsift-resolution`: 1 clippy error (`if`-collapse)
- `tsift-algorithms`: 6 clippy errors (too-many-args, complex-type, `or_insert_with`)

Not blockers for this mechanical lift; recorded as `[#cli-cleanup-unused-imports]` in the backlog.

#### Commits

- tsift @ `52e7cd1` — `cli: Phase 1b — extract clap CLI types into packages/tsift-cli/src/cli.rs` (pushed to `btakita/tsift` main)
- agent-loop @ `9c5dbd1` — submodule pointer bump (pushed)

VERSIONS.md `Unreleased` updated with the Phase 1b entry.

#### Top of backlog

`[#cli-cluster-index-search]` — Phase 2: move `cmd_index` + `cmd_search` bodies into `packages/tsift-cli/src/commands/index_search.rs`. Smallest cluster, validates the per-command pattern before larger ones.

### Re: do [#cli-cleanup-unused-imports] — opus-4-7

`make check` is green workspace-wide. Pre-existing clippy reds cleared across three crates:

**tsift-cli/src/lib.rs** — dropped unused `DEFAULT_BUDGET_BYTES`, `DEFAULT_BUDGET_ITEMS`, `DEFAULT_FOLLOW_UP_ITEMS` from the `output` import (left over from Phase 1a-i; the constants live in `output.rs` and are referenced there directly).

**tsift-resolution/src/scoring.rs** — dropped unused `GraphFreshness` import.

**tsift-algorithms** — five clippy errors:

- `coupling.rs:54-55` — `.or_insert_with(HashSet::new)` → `.or_default()` (clippy::unwrap_or_default)
- `dead_code.rs:89` — collapsed `if let Some(&idx) = node_idx.get(name) { if !reachable[idx] { ... } }` into `if let ... && !reachable[idx]` using let-chains
- `scc.rs:59` — Tarjan's inner recursive `strongconnect` closure has 8 params (state arrays passed by mutable ref). Added `#[allow(clippy::too_many_arguments)]` rather than wrapping in a struct — that's the idiomatic shape for Tarjan SCC and matches every reference implementation
- `health.rs:79` — same closure pattern, same `#[allow(clippy::too_many_arguments)]`
- `health.rs:26` — `build_graph` returns `(Vec<String>, HashMap<String, usize>, Vec<HashSet<usize>>, Vec<HashSet<usize>>)`. Added `#[allow(clippy::type_complexity)]`; introducing a named return struct here would only obscure that this is two adjacency maps plus a name⇄index pair

No behavior change in any of these.

#### Verification

- `make check` → exit 0 (cargo clippy `-D warnings` clean; cargo test passes; opencode-tsift npm publish dry-run ok)
- `cargo install --path packages/tsift-cli` — `~/.cargo/bin/tsift` refreshed for tmux sessions
- `cargo build --release -p tsift-cli` — `target/release/tsift` refreshed for the `.bin/` symlink

#### Commits

- tsift @ `1432872` — `cleanup: drop pre-existing clippy reds keeping make check red` (pushed)
- agent-loop @ `0fe1802` — submodule pointer bump (pushed)

VERSIONS.md `Unreleased` updated with the `#cli-cleanup-unused-imports` entry.

#### Top of backlog

`[#cli-cluster-index-search]` — Phase 2: move `cmd_index` + `cmd_search` bodies into `packages/tsift-cli/src/commands/index_search.rs`. Smallest cluster, validates the per-command pattern before the larger graph/quality/infra/digests clusters.

### Re: do [#cli-cluster-index-search] — opus-4-7

Phase 2 done — first per-command cluster lifted out of `lib.rs`.

#### What moved

Four functions (~800 LOC total) → `packages/tsift-cli/src/commands/index_search.rs`:

- `cmd_index` (~296 LOC) — `tsift index` dispatch incl. `--workspace`/`--submodule` fan-out, `--check`/`--rebuild`/`--prune` modes, json/compact/human renderers
- `cmd_search` (38 LOC) — thin default-budget wrapper, now `#[cfg(test)]`-gated in lib.rs since production paths call `cmd_search_with_budget` directly
- `cmd_search_with_budget` (~440 LOC) — the real `tsift search` engine: precheck/degraded-mode handling, scoped/federated dispatch, tagpath annotation, budget reporting, all output formats
- `cmd_search_worker` (24 LOC) — hidden `__search-worker` subcommand that writes a single sift result envelope to disk

Also introduced `packages/tsift-cli/src/commands.rs` as the parent module so the remaining clusters (graph, quality, summarize, infra, digests) can land alongside without rework.

#### Visibility changes (mechanical)

Promoted 30 crate-private helpers to `pub(crate)` so the child module can call them:

| Group | Helpers |
|-------|---------|
| Index | `run_index_update`, `relativize_index_summary`, `to_json_schema` |
| Strategy / precheck | `resolve_search_strategy`, `precheck_search_indexes`, `degraded_search_mode`, `emit_degraded_search_note`, `maybe_apply_search_post_precheck_test_hooks` (×2 cfg pair), `maybe_apply_search_worker_test_hooks` |
| Federated | `federated_symbol_search`, `federated_exact_search`, `federated_sift_search` |
| Relativization | `relativize`, `relativize_symbol_hits`, `relativize_json_paths` |
| Search runners | `run_exact_search_with_timeout`, `run_search_with_timeout`, `run_sift_search` |
| Budget / envelope | `build_search_budget_report`, `build_search_budget_follow_up`, `print_search_budget_human`, `print_json_or_envelope`, `envelope_metric`, `inject_tagpath_stale_into_json` |
| Rendering | `should_collapse_search_hits`, `group_search_hits`, `compact_snippet`, `format_score`, `abbreviate_match_type`, `abbreviate_kind` |

Plus `DegradedSearchMode` enum → `pub(crate)`. No bodies changed.

#### Verification

- `cargo build -p tsift-cli` — clean, no new warnings
- `cargo test -p tsift-cli` — **479 passed across 7 suites** (up from 298 lib + 129 integration = 427 the previous cycle; new suites picked up are unit tests under the moved file path)
- `make check` — green workspace-wide (clippy `-D warnings`, cargo test, opencode-tsift npm publish dry-run)
- `cargo install --path packages/tsift-cli` — `~/.cargo/bin/tsift` refreshed
- `cargo build --release -p tsift-cli` — `target/release/tsift` refreshed

#### Commits

- tsift @ `a69eefc` — `cli: Phase 2 — extract index+search command cluster into commands/index_search.rs` (pushed)
- agent-loop @ `4eceda5` — submodule pointer bump (pushed)

VERSIONS.md `Unreleased` updated with the `#cli-cluster-index-search` entry.

#### Pattern validated

This phase confirms the per-command-cluster pattern works at scale before tackling the larger groups: bodies move verbatim, helpers get `pub(crate)`, and the dispatch arms in `lib.rs::run()` keep referencing the same names through the import. Remaining Phase 2 clusters can follow the same recipe.

#### Top of backlog

`[#cli-cluster-graph]` — Phase 2: move `cmd_graph`, `cmd_communities`, `cmd_path`, `cmd_explain`, `cmd_explain_with_budget`, and `cmd_traverse` bodies into `packages/tsift-cli/src/commands/graph.rs`. Larger cluster (~6 functions); same recipe as this one.

### Re: do [#cli-cluster-index-search] — opus-4-7

Phase 2 done — first per-command cluster lifted out of `lib.rs`.

#### What moved

Four functions (~800 LOC total) → `packages/tsift-cli/src/commands/index_search.rs`:

- `cmd_index` (~296 LOC) — `tsift index` dispatch incl. `--workspace`/`--submodule` fan-out, `--check`/`--rebuild`/`--prune` modes, json/compact/human renderers
- `cmd_search` (38 LOC) — thin default-budget wrapper, now `#[cfg(test)]`-gated in lib.rs since production paths call `cmd_search_with_budget` directly
- `cmd_search_with_budget` (~440 LOC) — the real `tsift search` engine: precheck/degraded-mode handling, scoped/federated dispatch, tagpath annotation, budget reporting, all output formats
- `cmd_search_worker` (24 LOC) — hidden `__search-worker` subcommand that writes a single sift result envelope to disk

Also introduced `packages/tsift-cli/src/commands.rs` as the parent module so the remaining clusters (graph, quality, summarize, infra, digests) can land alongside without rework.

#### Visibility changes (mechanical)

Promoted 30 crate-private helpers to `pub(crate)` so the child module can call them: index helpers (`run_index_update`, `relativize_index_summary`, `to_json_schema`), search-strategy/precheck helpers (`resolve_search_strategy`, `precheck_search_indexes`, `degraded_search_mode`, `emit_degraded_search_note`, `maybe_apply_search_*_test_hooks`), federated search (`federated_symbol_search`, `federated_exact_search`, `federated_sift_search`), relativization (`relativize`, `relativize_symbol_hits`, `relativize_json_paths`), search runners (`run_exact_search_with_timeout`, `run_search_with_timeout`, `run_sift_search`), budget/envelope (`build_search_budget_report`, `build_search_budget_follow_up`, `print_search_budget_human`, `print_json_or_envelope`, `envelope_metric`, `inject_tagpath_stale_into_json`), and rendering (`should_collapse_search_hits`, `group_search_hits`, `compact_snippet`, `format_score`, `abbreviate_match_type`, `abbreviate_kind`).

`DegradedSearchMode` enum also promoted to `pub(crate)`. No bodies changed.

#### Verification

- `cargo build -p tsift-cli` — clean, no new warnings
- `cargo test -p tsift-cli` — **479 passed across 7 suites**
- `make check` — green workspace-wide (clippy `-D warnings`, cargo test, opencode-tsift npm publish dry-run)
- `cargo install --path packages/tsift-cli` — `~/.cargo/bin/tsift` refreshed
- `cargo build --release -p tsift-cli` — `target/release/tsift` refreshed

#### Commits

- tsift @ `a69eefc` — `cli: Phase 2 — extract index+search command cluster into commands/index_search.rs` (pushed)
- agent-loop @ `4eceda5` — submodule pointer bump (pushed)

VERSIONS.md `Unreleased` updated with the Phase 2 entry.

#### Pattern validated

This confirms the per-command-cluster pattern works at scale before tackling the larger groups: bodies move verbatim, helpers get `pub(crate)`, and the dispatch arms in `lib.rs::run()` keep referencing the same names through the import. Remaining Phase 2 clusters follow the same recipe.

#### Top of backlog

`[#cli-cluster-graph]` — Phase 2: move `cmd_graph`, `cmd_communities`, `cmd_path`, `cmd_explain`, `cmd_explain_with_budget`, and `cmd_traverse` bodies into `packages/tsift-cli/src/commands/graph.rs`. Larger cluster (~6 functions); same recipe as this one.

### Re: do [#cli-cluster-graph] — opus-4-7

Phase 2 cluster #2 done.

#### What moved

Six functions (~945 LOC total) → `packages/tsift-cli/src/commands/graph.rs`:

- `cmd_graph` (~269 LOC) — `tsift graph <symbol>` callers/callees with tagpath annotation, edge collapsing, scope dispatch
- `cmd_communities` (~148 LOC) — Louvain community detection over the call graph
- `cmd_traverse` (~30 LOC) — Graphify-style traversal graph dispatch (json + html)
- `cmd_path` (~86 LOC) — BFS shortest-path between two symbols
- `cmd_explain` (~31 LOC) — default-budget wrapper, now `#[cfg(test)]`-gated
- `cmd_explain_with_budget` (~381 LOC) — the real `tsift explain` engine with budget preview, tagpath annotation, and rendering

#### Visibility changes (mechanical)

Promoted 18 helpers + `CommunityDetectionReport` struct to `pub(crate)`:

| Group | Helpers |
|-------|---------|
| Traversal | `build_traversal_graph`, `traversal_report`, `traversal_report_html`, `verify_convex_projection_snapshot` |
| Community / explain | `build_explain_budget_report`, `community_tagpath_cache_part`, `compact_members`, `detect_communities_cached`, `print_explain_budget_human`, `update_community_annotation_diagnostics` |
| Edge rendering | `format_edge_groups`, `should_collapse_edge_groups` |
| Path/symbol | `symbol_path_summary` |
| Index / tagpath | `open_index_db`, `query_tagpath_root` |
| Relativization | `relativize_edges`, `relativize_symbols` |
| Misc | `shell_quote` |

No bodies changed.

#### Verification

- `cargo build -p tsift-cli` — clean, no new errors
- `make check` — green workspace-wide (clippy `-D warnings`, cargo test, opencode-tsift npm publish dry-run)
- `cargo install --path packages/tsift-cli` — `~/.cargo/bin/tsift` refreshed
- `cargo build --release -p tsift-cli` — `target/release/tsift` refreshed

#### Commits

- tsift @ `c6836c4` — `cli: Phase 2 — extract graph command cluster into commands/graph.rs` (pushed)
- agent-loop @ `f3b3c04` — submodule pointer bump (pushed)

VERSIONS.md `Unreleased` updated with the `#cli-cluster-graph` entry.

#### lib.rs trajectory

Phase 1a-i (output infra) → Phase 1a-ii (envelopes) → Phase 1b (CLI types) → Phase 2 cluster #1 (index+search) → Phase 2 cluster #2 (graph). lib.rs is now ~36.5k LOC, down from ~39.2k at Phase 1a-i kickoff. Two clusters left to land before tagpath helpers and the smaller infra/quality/summarize/digests clusters.

#### Top of backlog

`[#cli-extract-tagpath-helpers]` — Phase 1a-iii: extract `TagpathSearchOpts`, `TagpathAnnotationDiagnostic`, `CommunityMemberAmbiguityDiagnostic`, `annotate_hits_with_tagpath`, `annotate_stored_symbols_with_tagpath`, `annotate_stored_edges_with_tagpath`, `annotate_communities_with_tagpath`, `annotate_path_nodes_with_tagpath` (~700 LOC) into `packages/tsift-cli/src/output/tagpath.rs`. Sits below the graph/community cmd cluster (now drained) and moves cleanly because the annotate functions are already `pub`.
<!-- agent:boundary:c03e670e -->
<!-- /agent:exchange -->
###
<!--
What are #next-steps to make the graph faster?
---
What are #next-steps to make the graph + neighborhoods faster + more effective?
-->

<!-- agent:queue auto -->
preset #spec-test-build-install-commit-push
- ~do [#cli-extract-cli-types]~
- ~do [#cli-cleanup-unused-imports]~
- ~do [#cli-cluster-index-search]~
- ~do [#cli-cluster-graph]~
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
