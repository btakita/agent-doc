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

OpenCode npm package path code-complete and pushed. #gopencode-install and #gopencode gated on npm registry auth to publish opencode-tsift. 5 of 7 backlog items completed: integration tests, permission docs, transcript verify, smoke session, README expansion.
<!-- /agent:status -->

## Spec

See [src/tsift/SPEC.md](../../src/tsift/SPEC.md) for the AST-Aware Code RAG spec.

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc65443c45f7b49f5e6ace54e3f6bafe4077ade57924422b5eb3baf4dbd23c-20260528-222321.md`*

Compacted content:
- Archived 2 response topic(s): #step-4-graph; #step-6-resolution
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc6544...
❯ What are #next-steps to split out crates?

### Re: #next-steps for crate split — opus-4-7

**Current state.** The workspace already publishes 8 crates (`tsift-core`, `tsift-sqlite`, `tsift-libsql`, `tsift-graph`, `tsift-algorithms`, `tsift-resolution`, `tsift-tokensave`, `tsift-cli`). Root `tsift` still owns ~25k LOC across 27 modules. `SPEC.md` describes the *target* layout but root has not been drained yet.

**Remaining modules grouped by cohesion + dependency direction:**

| Proposed crate | Modules | LOC | Depends on |
|---|---|---|---|
| `tsift-sim-world` | sim_world | ~390 | tsift-core |
| `tsift-digest` | diff_digest, log_digest, metric_digest, test_digest | ~3.7k | tsift-core (emitters only) |
| `tsift-index` | index, walk, config, init | ~4.7k | tsift-core, tsift-sqlite, tsift-graph |
| `tsift-session` | session_cost, session_digest, session_review | ~7.1k | tsift-index, tsift-digest |
| `tsift-quality` | audit, lint, perf_gate, dci_benchmark, runtime_churn | ~3.7k | tsift-index, tsift-graph, tsift-algorithms |
| `tsift-search` | sift, summarize, impact, tagpath_adapter | ~3.2k | tsift-index, tsift-graph, tsift-resolution |

**Recommended order** (bottom-up by dependency, lowest blast radius first):

1. **#split-sim-world** — smallest + most isolated; validates the extraction pattern with minimal risk.
2. **#split-tsift-digest** — high-cohesion emitter group; few inbound deps from other root modules. Sets schema/serialization conventions reused by session.
3. **#split-tsift-index** — foundation for session/quality/search. Biggest dependency hub remaining in root; landing it unlocks the larger splits.
4. **#split-tsift-session** — largest LOC pool (~7.1k). Depends on digest + index, so sequence-after these.
5. **#split-tsift-quality** — gate surfaces (audit/lint/perf_gate). Independent of session; can land in parallel with #4 if index is stable.
6. **#split-tsift-search** — search/summarize/impact + tagpath_adapter. Final business-logic extraction.
7. **#demote-root-tsift** — once root `src/lib.rs` only re-exports, demote to thin shim or remove entirely; preserve `cargo install tsift` entrypoint.

**Also flagged:** `tsift-cli` is already 39k LOC in a single `lib.rs` — once root is drained, the next problem becomes the CLI monolith itself (`#split-tsift-cli-subcommands`, recommended).

**Cross-cutting checklist per split:**
- update `SPEC.md` Architecture block to mark the move from root → new crate
- add `[workspace.members]` entry and seed `lib.rs` with re-export shim from root for backward compat during transition
- thread feature flags (`lang-rust`, `backend-libsql`, etc.) through the new crate's `Cargo.toml` if it touches gated code
- run `cargo install --path .` + smoke `/tsift-status` after each split to verify no command path broke
- bump root `tsift` version on each landing (per release conventions) and update `VERSIONS.md`

**Top of backlog after this update:** `#split-sim-world`.

### Re: do [#split-sim-world] — blocker, gating for direction — opus-4-7

**Halting the queue.** I scoped #split-sim-world and the original backlog framing was wrong — the module is not a clean leaf. Gating this item and stopping the auto-loop before touching the remaining 7 split directives, because the same coupling concern probably re-surfaces for #split-tsift-digest / #split-tsift-session and the right architectural call should be a conversation.

**What I found:**

1. **`src/sim_world.rs` is currently orphaned.** It is NOT declared in `src/lib.rs` (root `tsift`). Root lib.rs lists every other module (`audit`, `config`, …, `walk`) but omits `sim_world`. So as it stands the file does not compile against any build target — it is dead code.

2. **It went stale during the tsift-cli extraction.** Commit `1786d0d Extract tsift-cli crate` moved `rewrite_command` into `tsift-cli` (where it is `pub(crate)`). `src/sim_world.rs:1` still imports `use crate::{init, rewrite_command, session_digest, status};` — `crate::rewrite_command` no longer exists at that path. The recent commit `580f5f1 test: run all default tests` only nudged a single line; the orphaning was never repaired.

3. **It is tightly coupled to private/test-only symbols in two crates:**
   - `tsift-cli::rewrite_command` — `pub(crate)`, lives in `packages/tsift-cli/src/lib.rs:37769`
   - `tsift::session_digest::extract_prompt_targets_from_text_block` — `#[cfg(test)] pub(crate)` in `src/session_digest.rs:899`
   - `tsift::init::{InstructionStatus, TSIFT_VERSION}` and `tsift::status::check_status` — already `pub`

   Two of those four targets are deliberately not part of either crate's public API. Promoting them to `pub` solely to support an external `tsift-sim-world` test crate inverts the usual rule (don't widen visibility for tests).

**Three options:**

| Option | What | Trade-off |
|---|---|---|
| **A.** Delete `src/sim_world.rs` | Acknowledge it has been broken/dead since the tsift-cli extraction and drop it. | Lose the deterministic sim coverage. Smallest change. Honest about current state. |
| **B.** Re-home tests adjacent to their targets | Move session/status sim-cases into `tsift` (under `#[cfg(test)] mod sim_world;` re-added to `src/lib.rs`) and the `rewrite_command` sim-cases into `packages/tsift-cli/tests/sim_world_rewrite.rs`. Keep `pub(crate)` boundaries; expose only what crate-internal tests already see. | Restores coverage. No new crate, but no public API widening either. Closes #split-sim-world by relocating, not extracting. |
| **C.** Force a `tsift-sim-world` crate per original backlog item | Promote `rewrite_command` and `extract_prompt_targets_from_text_block` to `pub`, add `packages/tsift-sim-world` depending on both, wire workspace member. | Widens public API for test-only callers. Adds one more workspace member. Matches the queue directive literally. |

**Recommendation: B.** It restores coverage that the tsift-cli extraction silently broke, keeps API surface narrow, and avoids re-using `tsift-sim-world` as a new crate name when the actual code is just test-relocation. After B lands, the backlog item closes as "done by relocation" and the remaining 7 splits (digest/index/session/quality/search/demote/cli-subcommands) — which DO look like genuine module-extraction targets — proceed unchanged.

**What I need from you:**

- Confirm A / B / C (or a hybrid).
- Decide whether to keep the rest of the queue active. If you pick **B**, I can run it next turn and the remaining 7 directives stay valid. If you pick **C**, I should re-scope each of the other 7 items for the same coupling concern before letting the auto-loop drive them.

**Why I am gating instead of pushing through:** the existing CLAUDE.md guidance for tsift says "All public logic tested: `classify_task`, `apply_edit_op`, `rewrite_command`, SQL helpers all have unit tests." That convention covers the rewrite_command tests, but it does not mandate cross-crate public exposure. Picking C without you signing off would silently widen the tsift-cli public API.

### Re: do [#split-tsift-digest] — blocker: cycle in proposed grouping — opus-4-7

**Halting the queue and reordering the backlog.** The original backlog ordering was grouped by naming-similarity, not by actual cross-module dependency. I ran a real import audit across the remaining root modules and found one cycle and one mis-ordered dependency chain.

#### Internal dep graph (root `tsift` modules → other root modules)

```
config:           (leaf)
lint:             (leaf)
runtime_churn:    (leaf)
perf_gate:        (leaf)
audit:            (leaf)
dci_benchmark:    (leaf)
metric_digest:    (leaf)
sift:             (leaf)
tagpath_adapter:  (leaf)
walk:             (leaf — `lang` is already external in tsift-graph)
init:             config
index:            walk
summarize:        index
session_cost:     runtime_churn
session_digest:   lint, runtime_churn
session_review:   lint, runtime_churn
test_digest:      lint, summarize
diff_digest:      lint, summarize
log_digest:       lint, runtime_churn, summarize
impact:           config, diff_digest, index, lint, summarize, walk
status:           config, index, init, summarize
```

#### Two problems with the original priority

1. **Cycle: proposed `tsift-search` ↔ `tsift-digest`.**
   - `diff_digest` uses `summarize` → tsift-digest depends on tsift-search.
   - `impact` (proposed for tsift-search) uses `diff_digest` → tsift-search depends on tsift-digest.
   - Cargo rejects this.

2. **Order inversion.** I had `#split-tsift-digest` second and `#split-tsift-quality` fifth. But `diff_digest`, `log_digest`, `test_digest`, `session_*` all import `lint` and `runtime_churn` from the proposed `tsift-quality`. So neither tsift-digest nor tsift-session can land until tsift-quality lands first.

#### Corrected order (dependency-bottom-up)

| # | Item | Reason |
|---|---|---|
| 1 | **#split-tsift-quality** | audit, lint, perf_gate, dci_benchmark, runtime_churn are leaves with zero internal deps. True first move. |
| 2 | **#split-tsift-index** | index → walk only. Foundation for summarize, session, search, status. |
| 3 | **#split-tsift-session** | Depends only on lint + runtime_churn → satisfied after #1. Independent of #2; can land in parallel. |
| 4 | **#split-tsift-search** [gated, in review] | Needs cycle decision below. |
| 5 | **#split-tsift-digest** [gated, in review] | Same cycle decision. |
| 6 | **#demote-root-tsift** | Once business crates land + `status`/`impact` placed, root becomes shim. |
| 7 | **#split-tsift-cli-subcommands** [recommended] | Independent CLI-side refactor. |

Backlog reordered. Top is now `#split-tsift-quality`.

#### Three options to break the digest ↔ search cycle

| Option | What |
|---|---|
| **X.** Move `impact` into `tsift-digest` | impact is essentially a higher-level digest (diff + symbol-level rollup); already uses diff_digest internally. Keeps tsift-search focused on retrieval/ranking/summaries. Cleanest. |
| **Y.** Promote `summarize` to its own `tsift-summarize` crate | Below both digest and search; both depend on it. More crates, separation matches actual responsibility. |
| **Z.** Merge proposed digest + search into one `tsift-derive` crate | Avoids cycle by not splitting. Largest single new crate (~7k LOC); least architectural improvement. |

**Recommendation: X.**

#### `status` placement

`status` depends on config + index + init + summarize. Under the original groupings, no proposed crate owns it. Options: leave in root tsift (problem for #demote-root-tsift), move to `tsift-cli` (it is essentially the `tsift status` command implementation), or new `tsift-status` crate. Recommend `tsift-cli`.

#### What I need from you

1. Confirm corrected order (or override).
2. Pick X / Y / Z for the digest↔search cycle.
3. Decide where `status` lives after the splits (root-shim vs tsift-cli vs new crate).
4. Re-author the queue with whichever splits you want me to drive. The auto-loop will not advance until you confirm direction. Once you queue `do [#split-tsift-quality]`, it should land cleanly.

### Re: do [#split-tsift-quality] — opus-4-7


**Done.** Extracted the four true-leaf modules into `packages/tsift-quality`. `lint` deferred — see below.

**Changed paths (tsift submodule, commit `43c1697`):**

- `packages/tsift-quality/Cargo.toml` (new)
- `packages/tsift-quality/src/lib.rs` (new — declares 4 modules)
- `packages/tsift-quality/src/audit.rs` (moved from `src/audit.rs`, 100% rename)
- `packages/tsift-quality/src/perf_gate.rs` (moved from `src/perf_gate.rs`, 100% rename)
- `packages/tsift-quality/src/dci_benchmark.rs` (moved from `src/dci_benchmark.rs`, 100% rename)
- `packages/tsift-quality/src/runtime_churn.rs` (moved from `src/runtime_churn.rs`, 100% rename)
- `Cargo.toml` — added `tsift-quality` to `[workspace.members]` and as root path dep
- `packages/tsift-cli/Cargo.toml` — added `tsift-quality = { path = "../tsift-quality" }`
- `src/lib.rs` — removed `pub mod audit; / dci_benchmark; / perf_gate; / runtime_churn;`; added `pub use tsift_quality::{audit, dci_benchmark, perf_gate, runtime_churn};` so `tsift::audit::*` / `tsift::runtime_churn::*` paths used by `tsift-cli` (~10 callers) and the remaining root modules (`log_digest`, `session_*`) keep resolving without source edits.
- `SPEC.md` — Architecture diagram updated with `tsift-quality crate (packages/tsift-quality — quality-gate surfaces)`.
- `VERSIONS.md` — `Unreleased` entry added.
- `.gitignore`, `Cargo.lock` — incidental.

**Commands run:**
- `cargo build --release` — 0 errors, 3 warnings (all pre-existing).
- `cargo test --release --no-fail-fast` — **277 passed** across two suites.
- `cargo install --path packages/tsift-cli` — installed `tsift 0.1.62` to `~/.cargo/bin/`.
- `tsift --version` → `tsift 0.1.62`.
- `tsift status` → ran cleanly; flagged routine stale-index (35m old).

**Repo state:**
- tsift submodule HEAD: `43c1697 Extract tsift-quality crate — move audit, perf_gate, dci_benchmark, runtime_churn` (pushed)
- Superproject HEAD: pointer bump pushed.

**Lint deferred.** During scoping I found `lint` is not a true leaf — it imports `crate::{config, index::IndexDb}` and is called from 6 root modules (`diff_digest`, `impact`, `log_digest`, `session_digest`, `session_review`, `test_digest`) plus ~10 sites in `tsift-cli` (via `tsift::lint::resolve_project_root_or_canonical_path`). Moving it into `tsift-quality` would require it to depend on the next crate in the queue, so I left it in root for this cycle. It joins `tsift-quality` after that next crate lands.

**Confidence:** high — full build + 277 tests pass; install verified; no API surface widened (all moved symbols stay reachable through root tsift re-exports).

**Escalation:** none.

### Re: do [#split-tsift-index] — opus-4-7


**Done.** Extracted `config`, `walk`, `init`, and `index` from root `tsift` into the new `tsift-index` workspace crate.

**Changed paths (tsift submodule):**

- `packages/tsift-index/Cargo.toml` (new — version `0.1.62` to keep `TSIFT_VERSION = env!("CARGO_PKG_VERSION")` aligned with the binary; deps: anyhow, serde, serde_json, toml, rusqlite, fs4, ignore, tagpath, tsift-graph, tsift-sqlite; dev: tempfile)
- `packages/tsift-index/src/lib.rs` (new — declares 4 modules)
- `packages/tsift-index/src/config.rs` (moved from `src/config.rs`, 100% rename)
- `packages/tsift-index/src/walk.rs` (moved + import fix: `crate::lang::Lang` → `tsift_graph::lang::Lang`)
- `packages/tsift-index/src/init.rs` (moved; test path fix: `manifest_dir.join("packages/opencode-tsift")` → `manifest_dir.join("../opencode-tsift")` since `CARGO_MANIFEST_DIR` now points at the crate, not root)
- `packages/tsift-index/src/index.rs` (moved + import fixes: `use crate::graph` → `use tsift_graph as graph`, `use crate::lang::Lang` → `use tsift_graph::lang::Lang`; test block: `crate::substrate::*` → `tsift_sqlite::*`; promoted `WriterLockProbe` enum and `probe_writer_lock` fn from `pub(crate)` to `pub` so `status` in root tsift can keep importing them)
- `Cargo.toml` — added `tsift-index` to `[workspace.members]` and as root path dep
- `packages/tsift-cli/Cargo.toml` — added `tsift-index = { path = "../tsift-index" }`
- `packages/tsift-cli/tests/perf_gate.rs` — replaced `#[path = "../../../src/perf_gate.rs"] mod perf_gate;` (now-broken include) with `use tsift_quality::perf_gate;` (proper crate dep)
- `packages/tsift-quality/Cargo.toml` — added `[dev-dependencies] tempfile = "3"` so audit tests build under `--workspace`
- `src/lib.rs` — removed `pub mod config; / index; / init; / walk;`; added `pub use tsift_index::{config, index, init, walk};`
- `SPEC.md` — added `tsift-index crate (packages/tsift-index — config + project walk + init + AST symbol index)` to Architecture
- `VERSIONS.md` — `Unreleased` entry added
- `Cargo.lock` — regenerated

**Commands run:**
- `cargo build --release` — 0 errors after iteration (first pass surfaced missing `serde_json` + `tsift-sqlite` deps in `tsift-index`; added; clean after).
- `cargo test --release --no-fail-fast --workspace` — **990 passed across 27 suites**. First pass surfaced two failures, both fixed before commit: `tempfile` missing dev-dep on tsift-quality, and `TSIFT_VERSION` falling out of sync because tsift-index had `version = "0.1.0"`.
- `cargo install --path packages/tsift-cli` — installed `tsift 0.1.62` to `~/.cargo/bin/`.
- `tsift --version` → `tsift 0.1.62`.

**Repo state:**
- tsift submodule commit: `Extract tsift-index crate — move config, walk, init, index` (pushed).
- Superproject commit: `216a5f1a4 tsift: extract tsift-index crate (submodule pointer bump)` (pushed).

**Visibility widened:** `WriterLockProbe` + `probe_writer_lock`. Both are now `pub` in `tsift-index`. Only one caller (`status` in root) needed the access; the surface widens by exactly two symbols. Acceptable, given lint/status will both move in the same direction once tsift-search/quality round trips finish.

**Confidence:** high — full workspace test sweep green, install verified, no caller breakage.

**Escalation:** none.

### Re: do [#split-tsift-session] — opus-4-7


**Done.** Extracted `session_cost`, `session_digest`, `session_review` (~7.1k LOC, largest single root pool) into the new `tsift-session` crate. Required a prereq move of `lint` into `tsift-quality`, folded into the same cycle.

**Why the lint ride-along:** the moved session files call `crate::lint::resolve_harness_root_or_canonical_path` inline. After move, `crate::lint` wouldn't exist in `tsift-session`. Two paths to fix:

1. Make `tsift-session` depend on root `tsift`. But root re-exports `session_*` from `tsift-session` → cycle.
2. Move `lint` to a crate `tsift-session` can depend on cleanly.

Took #2: lint's only deps (`config`, `index::IndexDb`) already live in `tsift-index`. `tsift-quality` now depends on `tsift-index` for that purpose. No cycle. tsift-session → tsift-quality → tsift-index → tsift-graph.

**Changed paths (tsift submodule):**

- `packages/tsift-session/Cargo.toml` (new — version `0.1.62`; deps: anyhow, serde, serde_json, tsift-quality; dev: tempfile)
- `packages/tsift-session/src/lib.rs` (new — declares 3 modules)
- `packages/tsift-session/src/session_cost.rs` (moved + `crate::runtime_churn::*` → `tsift_quality::runtime_churn::*`)
- `packages/tsift-session/src/session_digest.rs` (moved + `crate::runtime_churn::*` → `tsift_quality::runtime_churn::*`, `crate::lint::resolve_harness_root_or_canonical_path` → `tsift_quality::lint::resolve_harness_root_or_canonical_path`)
- `packages/tsift-session/src/session_review.rs` (moved + same lint/runtime_churn rewrites)
- `packages/tsift-quality/src/lint.rs` (moved from root `src/lint.rs`; `crate::{config, index::IndexDb}` → `tsift_index::{config, index::IndexDb}`)
- `packages/tsift-quality/Cargo.toml` — added `tsift-index` path dep + `rusqlite` dev-dep (lint tests need it)
- `packages/tsift-quality/src/lib.rs` — added `pub mod lint;`
- `Cargo.toml` — added `tsift-session` workspace member and root path dep
- `packages/tsift-cli/Cargo.toml` — added `tsift-session = { path = "../tsift-session" }`
- `src/lib.rs` — removed `pub mod log_digest;` block stayed, removed `pub mod session_cost; / session_digest; / session_review;` and `pub mod lint;`; added `pub use tsift_quality::{..., lint, ...};` and `pub use tsift_session::{session_cost, session_digest, session_review};`
- `SPEC.md` — added `tsift-session crate` block; updated `tsift-quality` block to mention `lint` + tsift-index dep
- `VERSIONS.md` — `Unreleased` entry added
- `Cargo.lock` — regenerated

**Commands run:**
- `cargo build --release` — 0 errors after one round (first pass missed `rusqlite` dev-dep on `tsift-quality`; added).
- `cargo test --release --no-fail-fast --workspace` — **990 passed across 29 suites**.
- `cargo install --path packages/tsift-cli` — installed `tsift 0.1.62`.

**Repo state:**
- tsift submodule: `Extract tsift-session crate + fold lint into tsift-quality` (pushed, HEAD `55f1525`).
- Superproject: `7900d6972 tsift: extract tsift-session crate + fold lint into tsift-quality` (pushed).

**Visibility:** no new public symbols. `crate::lint::resolve_harness_root_or_canonical_path` was already `pub` before the move; it just relocated.

**Confidence:** high — 990 tests green, install verified.

**Escalation:** none.
<!-- agent:boundary:20ccf529 -->
<!-- /agent:exchange -->
###
<!--
What are #next-steps to make the graph faster?
---
What are #next-steps to make the graph + neighborhoods faster + more effective?
-->

<!-- agent:queue auto -->
preset #spec-test-build-install-commit-push
- ~do [#split-sim-world]~
- ~do [#split-tsift-digest]~
- ~do [#split-tsift-quality]~
- ~do [#split-tsift-index]~
- ~do [#split-tsift-session]~
- ~do [#split-tsift-quality]~
- ~do [#split-tsift-search]~
- do [#demote-root-tsift]
- do [#split-tsift-cli-subcommands]
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog -->
- [ ] [#demote-root-tsift] After all module splits land, demote root tsift crate to a thin re-export shim (or remove lib.rs entirely) — leave only the workspace root + binary metadata; verify external consumers (cargo install tsift) keep working
- [ ] [#split-tsift-cli-subcommands] [recommended] tsift-cli is ~39k LOC in a single lib.rs — propose per-command module split (cmd_search, cmd_index, cmd_graph, cmd_explain, etc.) once root tsift is drained, to make command surface independently testable
- [ ] [#npm-auth-setup] Configure NPM_TOKEN secret + TSIFT_ENABLE_NPM_PUBLISH=true variable in GitHub repo settings
- [ ] [#npm-publish-verify] After auth configured, tag release and verify publish-opencode-plugin CI job succeeds and opencode-tsift appears on npmjs.org
- [ ] [#full-e2e-verify] In fresh project: opencode plugin opencode-tsift && opencode then /tsift-status. On success mark #e2e-install done.
<!-- /agent:backlog -->

## Review

<!-- agent:review -->
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
