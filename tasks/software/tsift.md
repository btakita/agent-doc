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

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc65443c45f7b49f5e6ace54e3f6bafe4077ade57924422b5eb3baf4dbd23c-20260529-072627.md`*

Compacted content:
- Archived 7 response topic(s): do [#cli-cluster-infra]; do [#cli-cluster-digests]; What are #next-steps to complete the review items?; 4 more
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc6544...

### Re: do A — #split-sim-world via Option A (test-harness crate) — opus-4-8

Done, committed, pushed, CI green. Option A re-homed the SimWorld harness as a test-harness crate **and** fixed the dead-test bug — which turned out to have a third root cause and a wider blast radius than the investigation surfaced.

#### What landed → `packages/tsift-sim-world`

A new workspace crate whose `src/lib.rs` carries **no production surface** — the deterministic seeded-corpus + named-edge-trace tests live in `tests/sim_world.rs`, with `tsift` + `tsift-cli` as **dev-dependencies**. Dev-deps don't form a lib cycle, so root `tsift` does not (and cannot) re-export it. `src/sim_world.rs` is deleted (git tracked it as a rename).

#### Three root causes the harness had silently rotted on

The investigation found two; building it surfaced a third:

1. **No `mod sim_world`** anywhere → never compiled. (known)
2. **`crate::rewrite_command`** now lives in `tsift-cli` as `pub(crate)` → unreachable from root. (known) → **promoted `tsift_cli::rewrite_command` to `pub`.**
3. **`session_digest::extract_prompt_targets_from_text_block`** became `#[cfg(test)] pub(crate)` in `tsift-session` after the session split → invisible cross-crate, so the harness couldn't compile even as a sibling. **(new)** → added a **`test-support` feature** to `tsift-session` that re-exposes it as `pub` (`#[cfg(any(test, feature = "test-support"))]`), enabled by `tsift-sim-world`'s dev-dependency. No production bloat — the feature is off in every non-harness build.

This confirms the review item's literal "root `tsift` re-exports via `pub use`" framing was **impossible** (cycle), and Option A's dev-dependency shape is the only form that both extracts and re-activates it.

#### The wider blast radius — `make check` wasn't running these tests at all

The harness was dead because `make check`'s `test:` target ran **bare `cargo test`** = root `tsift` package only (56 tests). It never ran `tsift-cli`'s 479 tests, the 3 sim tests, or `tsift-resolution`'s suite. **Switched `test:` → `cargo test --workspace`.**

That immediately caught **one more stranded dead test**: `tsift-resolution/src/scoring.rs` used `GraphFreshness` without importing it. The irony — a prior `#cli-cleanup-unused-imports` cycle had *removed* that import as "unused" precisely because the test never compiled under root-only `cargo test`. Re-added `use tsift_core::GraphFreshness`. The whole class is now guarded going forward.

#### Verification

- `cargo test --workspace` — **993 passed across 34 suites** (was effectively 56 reaching CI before)
- `make check` — green (clippy `-D warnings` clean, workspace tests, opencode npm dry-run); confirmed `Running tests/sim_world.rs` now executes
- `cargo install --path packages/tsift-cli` + `cargo build --release -p tsift-cli` — both binaries refreshed; `tsift --version` → `0.1.62`
- **CI green** — run `26623438920`, both `clippy + test` and `live Convex graph acceptance` jobs succeeded (CI runs `make ci-full` → the same `cargo test --workspace` path)

#### Commits

- tsift @ `7f0e1c6` — `#split-sim-world` (pushed)
- agent-loop @ `2eed5dbdb` — submodule pointer bump (pushed)

VERSIONS.md `Unreleased` updated. Marked `[#split-sim-world]` **done**.

#### Top of backlog

Next actionable: **`[#split-tsift-digest]`** — extract `diff_digest` + `log_digest` + `metric_digest` + `test_digest` into a `tsift-digest` crate. Low-risk: `tsift-cli` already calls these as `tsift::diff_digest::*`, so crate + root re-export keeps callers compiling. Then `#split-tsift-search` → `#demote-root-tsift`; the npm chain (`#npm-auth-setup` →) stays externally blocked on your GitHub repo-settings access.
### Re: dispatch #spec-test-build-install-commit-push → do [#split-tsift-digest] / [#split-tsift-search] / [#demote-root-tsift] — opus-4-8

All three crate splits done, sequentially (digest → search → demote-root, respecting the demote dependency), each with spec + tests updated, build + install, commit + push, and CI green. The root `tsift` crate is now a thin re-export shim.

Note: your queued `do [#…]` items were auto-struck by preflight as `source=review_gated` (each id existed both as an open `[ ]` backlog item *and* a gated `[/]` review proposal; the queue resolver keyed on the gated copy and halted). I confirmed with you and ran them as the actionable backlog work.

#### 1. `#split-tsift-digest` → `packages/tsift-digest`

Moved `diff_digest` + `log_digest` + `metric_digest` + `test_digest` (~3.7k LOC) out of root `tsift/src/`. Root re-exports `pub use tsift_digest::{diff_digest, log_digest, metric_digest, test_digest};`. Rewrote crate-internal imports: `crate::graph` → `tsift_graph`, `crate::lang::Lang` → `tsift_graph::lang::Lang`, `crate::lint`/`crate::runtime_churn` → `tsift_quality::{lint,runtime_churn}`, `crate::summarize::{SummaryDb,Summary}` → `tsift_summarize::summarize::{…}`. tsift `6144fcd`, CI `26624750805` ✓.

#### 2. `#split-tsift-search` → `packages/tsift-search`

Moved `sift` (lexical adapter) + `impact` (change-impact) + `tagpath_adapter` (`summarize` already in tsift-summarize). Root re-exports `pub use tsift_search::{impact, sift, tagpath_adapter};`. `sift`/`tagpath_adapter` were self-contained; `impact` rewired to `tsift_index`/`tsift_digest`/`tsift_graph`/`tsift_quality`/`tsift_summarize`. `impact::is_import_line` gates per-`Lang` arms on `lang-*`, so tsift-search defines the full `lang-*` set forwarding to tsift-graph, and root forwards `lang-* → tsift-search/lang-*`. tsift `417bb80`, CI `26625103762` ✓.

#### 3. `#demote-root-tsift` → root is now a thin shim

Extracted the last real module, `status` (~2.07k LOC, backs `tsift status` + `tsift locks`), into `packages/tsift-status`; root re-exports `pub use tsift_status::status;`. **Root `tsift/src/` now holds only `lib.rs` + five one-line re-export shims** (`graph`, `lang`, `resolution`, `substrate`, `libsql_backend`). Since root has no code of its own, I **pruned all external crate deps** (anyhow, clap, rusqlite, serde, tree-sitter, tagpath, …) + unused `tsift-core`/`tsift-algorithms`; root keeps only the `tsift-*` path deps it re-exports + optional `tsift-libsql`. Relocated `examples/dump_ast.rs` → `packages/tsift-graph/examples/`. Verified default + `--features backend-libsql` builds and the `cargo install` binary. tsift `2c75eb3`, CI `26625522294` ✓.

#### Verification (each split)

- `make check` green every time (clippy `-D warnings`, `cargo test --workspace`, opencode npm dry-run)
- Workspace tests held at **993 passed** throughout — 34 → 36 → 38 → 40 suites as each crate's lib+doctest binaries were added; the per-module tests moved with their code, so the total never dropped
- both binaries refreshed each cycle (`tsift --version` → `0.1.62`); smoke-tested `diff-digest`, `search`, `status` via the re-exports
- agent-loop submodule pointers bumped + pushed each split (`0c3b02797`, `ba891cf3b`, `f8d65122a`)

#### Backlog status

`#split-tsift-digest`, `#split-tsift-search`, `#demote-root-tsift` → **done**. The per-command CLI split (`#split-tsift-cli-subcommands`) was effectively completed earlier this session and stays as the gated review record (not marking it done unilaterally — say the word). Remaining backlog is the **OpenCode npm publish chain** (`#npm-auth-setup` → `#npm-publish-verify` → `#full-e2e-verify`), still externally blocked on your GitHub repo-settings access (`NPM_TOKEN` secret + `TSIFT_ENABLE_NPM_PUBLISH=true`) — **not** completed here. **Top of backlog: `#npm-auth-setup`** (blocked — needs you).

The whole crate-extraction arc is now complete: root `tsift` went from a ~39k-LOC monolith to a thin re-export shim over focused `tsift-*` workspace crates.
❯ Please evaluate the extraction. Is the test suite working as it should? Are there any regressions that need to be fixed? Add #next-steps
### Re: Please evaluate the extraction — is the test suite working, any regressions? — opus-4-8

**Verdict: the extraction is clean — no functional regressions, test suite working correctly.** I did find one real *non-extraction* issue (a clippy coverage gap) and fixed the coupled parts; the rest is captured as a new top-of-backlog item.

#### Is the test suite working as it should? — Yes

- `cargo test --workspace`: **993 passed across 40 suites, 0 failed, 0 ignored.** No `#[ignore]` anywhere.
- The per-module tests **moved with their code** — the workspace total held at 993 the whole way (34 → 36 → 38 → 40 suites as each new crate's lib+doctest binaries were added). Nothing was silently dropped.
- The dead-test gap that *started* this whole arc is closed: `make check`'s `test:` runs `cargo test --workspace` (was root-only). I re-verified each split's tests actually execute (e.g. `Running tests/sim_world.rs`, `tsift_digest`, `tsift_status` binaries).
- CI green on every split (`26624750805`, `26625103762`, `26625522294`).

#### Are there regressions from the extraction? — No

- Public API preserved: every moved module is re-exported from root `tsift` (`pub use tsift_{digest,search,status}::…`), so `tsift::diff_digest::*` / `tsift::sift::*` / `tsift::status::*` callers in `tsift-cli` compile unchanged. Smoke-tested `diff-digest`, `search`, `status` through the installed binary — all work.
- No dependency cycles; default + `--features backend-libsql` both build; `cargo install` binary verified.
- Minor note (not a regression): the digest/search crates depend on `tsift-graph` with default (all-language) features, matching the existing `tsift-index` precedent rather than forwarding root's `lang-*` selection. Behavior is identical for the default build; only a non-default `--no-default-features` consumer would see all grammars in the digest path. Pre-existing pattern, left as-is.

#### The one real finding: a clippy coverage gap (mirror of the dead-test bug)

`make check`'s clippy leg runs `cargo clippy --all-targets` against the **root package only** — exactly like `test:` was before `#split-sim-world`. `cargo clippy --workspace` surfaces pre-existing lint debt that was never checked. I cleaned the cheap/coupled crates and pushed them (CI green):

- **tsift-libsql** — dropped unused imports; **migrated the deprecated `libsql::Database::open(...)` → `block_on(&rt, Builder::new_local(...).build())`** (matches the existing `open_remote` path); dropped two `unused_mut`; scoped test-only `Graph{Freshness,Projection,Provenance}` into the test module.
- **tsift-tokensave** — dropped unused `VecDeque` + `Graph*` imports.
- **tsift-resolution** — three `useless vec!` → arrays in `scoring.rs` tests (these became *visible* because `#split-sim-world`'s `GraphFreshness` test-import fix made that test compile again).

Commit `4f7f053` (CI green — the first run hit a transient infra failure where the runner died during the cache step *before* `make ci-full` ran; the "Full CI suite" step never started; re-run passed clean).

I **did not** flip `make check`'s clippy to `--workspace` yet, because that requires first clearing `tsift-cli`'s remaining debt (9 `dead_code` `graph_db_*` helpers flagged unused in both lib and lib-test, plus one `too_many_arguments`) — a delete-vs-gate decision on someone's diagnostic helpers that deserves its own focused pass rather than getting buried here. Captured as `#next-steps` below.

#### #next-steps

Added **`[#clippy-workspace-gate]`** to the top of the backlog (actionable; ranked above the externally-blocked npm chain). The backlog is now in priority order:

1. **`#clippy-workspace-gate`** ← top (actionable) — clear tsift-cli lint debt, then flip make-check clippy to `--workspace`.
2. `#npm-auth-setup` (externally blocked — needs your GitHub repo-settings access)
3. `#npm-publish-verify` (blocked on #2)
4. `#full-e2e-verify` (blocked on #2/#3)

**Top of backlog: `#clippy-workspace-gate`.**
<!-- agent:boundary:954833c7 -->
<!-- /agent:exchange -->
###
<!--
What are #next-steps to make the graph faster?
---
What are #next-steps to make the graph + neighborhoods faster + more effective?
-->

<!-- agent:queue -->
dispatch #spec-test-build-install-commit-push
- ~do [#split-tsift-digest]~
- ~do [#split-tsift-search]~
- ~do [#demote-root-tsift]~
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
