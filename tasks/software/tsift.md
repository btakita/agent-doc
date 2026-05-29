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
❯ What are #next-steps to complete the review items?
### Re: What are #next-steps to complete the review items? — opus-4-8

First — I filed the `#agent-doc-bug` you reported (prompt duplicated while typing) to `tasks/agent-doc/agent-doc-bugs2.md` as `[#prompt-duplicated-while-typing]` with a full plan at `tasks/agent-doc/plan-prompt-duplicated-while-typing.md` (root-cause hypothesis + Option A/B fix). It's a distinct symptom from the two existing IPC-duplication bugs. Details at the bottom.

Now the tsift review items. They split into two tracks:

#### Track A — crate-split architecture (agent-actionable now)

I checked `src/tsift/` against the review items:

- **`#split-tsift-cli-subcommands` — effectively DONE this session.** The Phase 1/2 work split `tsift-cli/lib.rs` (39.2k → 32.8k LOC) into `commands/{index_search,graph,quality,summarize,infra,digests}.rs` + `cli.rs` + `output/tagpath.rs`. The command surface is now modular. **Recommend marking this review item done** (your call — it's gated).
- **`#split-sim-world` — not done.** `src/sim_world.rs` (~390 LOC) still in root. Smallest, most isolated → the right next step to validate the crate-extraction pattern.
- **`#split-tsift-digest` — not done.** `diff_digest.rs`, `log_digest.rs`, `metric_digest.rs`, `test_digest.rs` still in root `src/`. Now low-risk because `tsift-cli`'s digests cluster already calls these as `tsift::diff_digest::*` etc., so a crate + root re-export keeps callers compiling.
- **`#split-tsift-search` — not done.** `sift.rs`, `impact.rs`, `tagpath_adapter.rs` still in root (`summarize` already lives in `tsift-summarize`). Depends on `tsift-index` + `tsift-graph` (both exist).
- **`#demote-root-tsift` — blocked** until the three splits above land; then root `tsift` becomes a thin re-export shim (verify `cargo install tsift` consumers).

Suggested order (low-risk → unblocking): `#split-sim-world` → `#split-tsift-digest` → `#split-tsift-search` → `#demote-root-tsift`.

These are gated in `agent:review` deliberately (proposals). I have **not** promoted them to the active backlog unilaterally — say the word and I'll promote them in that order and start with `#split-sim-world`.

#### Track B — OpenCode npm publish (externally blocked)

`#npm-auth-setup` → `#npm-publish-verify` → `#full-e2e-verify` / `#e2e-install` form a chain gated on **`#npm-auth-setup`**, which needs GitHub repo-settings access (set `NPM_TOKEN` secret + `TSIFT_ENABLE_NPM_PUBLISH=true` variable). That's a human/admin step I can't do from here — it blocks the rest of Track B. If you run it (or tell me the token's in place), I can drive `#npm-publish-verify` onward.

#### Backlog status

Top of the current `agent:backlog`: **`[#npm-auth-setup]`** — but it's externally blocked, so the *actionable* next step is Track A's `#split-sim-world`. The 3-item backlog (auth → publish-verify → full-e2e) is already in dependency order. If you want Track A prioritized, I'll promote those review items above the npm chain.

#### Re: the duplication bug (filed)

Symptom matches this session's recurring `live_prompt_drift_after_preflight` / `mid-turn cycle rotation` / `already_applied` IPC diagnostics. Hypothesis (unproven, flagged as such in the plan): the editor-side normalize+`setText` in `PatchWatcher` re-incorporates post-boundary keystrokes that the Rust-side `try_ipc()` boundary-aware synthesized exchange patch *also* carries, so `setText(result)` writes the typed prompt twice; the `await_idle` debounce narrows but doesn't close the race. Recommended fix (Option A): make the post-boundary user region single-source (editor buffer authoritative; exclude live user text from the synthesized patch) + idempotent apply keyed on (patch_id, user-region-hash) — which would also harden the two related IPC-duplication bugs.
❯ Add #next-steps to complete review items.
### Re: Add #next-steps to complete review items. — opus-4-8

Added the actionable next-steps to `agent:backlog`, promoted from the gated `agent:review` proposals and ordered by priority (lowest-risk/unblocking first, externally-blocked last).

**Backlog now (priority order):**

1. **`[#split-sim-world]`** ← top — extract `src/sim_world.rs` (~390 LOC) into `tsift-sim-world`. Smallest/most-isolated; validates the crate-extraction pattern.
2. `[#split-tsift-digest]` — extract `diff_digest`+`log_digest`+`metric_digest`+`test_digest` into `tsift-digest`. Low-risk: `tsift-cli` already calls these as `tsift::diff_digest::*`, so crate + root re-export keeps callers compiling.
3. `[#split-tsift-search]` — extract `sift`+`impact`+`tagpath_adapter` into `tsift-search` (`summarize` already in `tsift-summarize`). Deps `tsift-index`+`tsift-graph` exist.
4. `[#demote-root-tsift]` — after the three splits land, demote root `tsift` to a thin re-export shim; verify `cargo install tsift`.
5. `[#npm-auth-setup]` — **externally blocked** (needs your GitHub repo-settings access: `NPM_TOKEN` secret + `TSIFT_ENABLE_NPM_PUBLISH=true`).
6. `[#npm-publish-verify]` — blocked on #5.
7. `[#full-e2e-verify]` — blocked on #5/#6.

**Top of backlog: `#split-sim-world`.**

Notes:
- The four crate-split items remain in `agent:review` as the proposal record; I added active backlog entries (same ids) tagged "promoted from agent:review".
- `[#split-tsift-cli-subcommands]` is effectively done (this session's Phase 1/2 modularization of `tsift-cli/lib.rs`) — I did not add a backlog item for it and left it gated in `agent:review`; say the word and I'll mark that review item done.
- Track A (#1–#4) is agent-actionable now; want me to start on `#split-sim-world`?
❯ yes
### Re: yes (start #split-sim-world) — opus-4-8

Stopped before implementing — investigation found the review item rests on a **stale premise**, and the naive form would break the build. Two blockers:

**1. `sim_world.rs` is currently orphaned dead code.** There is no `mod sim_world` declaration anywhere (`src/lib.rs`, `main.rs`, Cargo targets, `include!`, `build.rs` — all checked). The module isn't compiled, and its three SimWorld tests (`sim_world_fast_seed_corpus_*`, `sim_world_medium_seed_corpus_*`, `sim_world_named_edge_trace_*`) are **not registered in any test binary** (`cargo test -p tsift --lib -- --list` shows none). It was silently de-wired when its dependencies moved out of root `tsift` during the earlier crate splits. So "the SimWorld coverage" the CI-review rule references isn't actually running.

**2. The "root tsift re-exports" framing creates a dependency cycle.** `sim_world.rs` imports `crate::{init, rewrite_command, session_digest, status}` — `status` + `rewrite_command` still live in root `tsift`. A `tsift-sim-world` **lib** crate that root `tsift` re-exports would be `tsift → tsift-sim-world → tsift` — a cycle cargo rejects.

#### Options (your call — I recommend A)

- **A (recommended): re-home as a test-harness crate.** Create `tsift-sim-world` as a crate whose SimWorld harness depends on `tsift` (+ `tsift-index`, `tsift-session`) as a **dev-dependency**, with the tests in it. Dev-deps don't form a lib cycle, root `tsift` does **not** re-export it, and `make check` runs it via `cargo test -p tsift-sim-world`. This both extracts it *and* fixes the dead-test bug. Closest to the review item's intent, minus the (impossible) re-export.
- **B: re-wire in place first, extract later.** Add `#[cfg(test)] mod sim_world;` to root `tsift/src/lib.rs` to re-activate the tests where they are; defer the crate move until `status`/`rewrite_command` are themselves extracted (which is really the `#demote-root-tsift` endgame). Lower churn, keeps the cycle question for later.
- **C: delete the orphan.** Only if SimWorld is obsolete — but the project's CI-review rule explicitly values SimWorld coverage, so I doubt it.

I left `#split-sim-world` **open** in the backlog (not started) pending your pick. Note the dead-test finding is arguably its own bug worth tracking regardless of which option — the harness silently stopped running.

Which option?
do A
<!-- agent:boundary:f67635b7 -->
<!-- /agent:exchange -->
###
<!--
What are #next-steps to make the graph faster?
---
What are #next-steps to make the graph + neighborhoods faster + more effective?
-->

<!-- agent:queue auto -->
- do A
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog -->
- [ ] [#split-sim-world] Extract sim_world (~390 LOC, src/sim_world.rs) into a new tsift-sim-world crate — smallest/most-isolated module, validates the crate-extraction pattern; root tsift re-exports via pub use. (promoted from agent:review)
- [ ] [#split-tsift-digest] Extract diff_digest + log_digest + metric_digest + test_digest (src/, ~3.7k LOC) into a new tsift-digest crate. tsift-cli already calls these as tsift::diff_digest::* etc., so crate + root re-export keeps callers compiling. (promoted from agent:review)
- [ ] [#split-tsift-search] Extract sift + impact + tagpath_adapter (src/) into a new tsift-search crate (summarize already in tsift-summarize). Depends on tsift-index + tsift-graph (both exist). (promoted from agent:review)
- [ ] [#demote-root-tsift] After the sim-world/digest/search splits land, demote root tsift to a thin re-export shim (or remove lib.rs); verify cargo install tsift consumers keep working. (promoted from agent:review)
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
