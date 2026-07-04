---
name: project_agent_doc_module_decomposition
description: "Ongoing agent-doc large-module decomposition program (functionality + inline tests), progress + proven pipeline"
metadata: 
  node_type: memory
  type: project
  originSessionId: 5f221ae6-1975-4b37-b8be-22a4b6f30d63
---

Operator wants every large agent-doc module split into **coherent functional submodules, each bundling its tests inline** (NO separate `<mod>/tests.rs` files). See [[feedback_module_split_functionality_with_tests]]. Commits use `refactor(splitmods3/4): ...`.

**Done (verified `make check` green) as of 2026-06-12 — 25 commits `splitmods3/4`:**
- `write.rs` 28,006 → 9 modules (queue_consume, ipc, normalize, converge, pending_checks, materialize, exchange_reconcile, run_entry + core), tests distributed inline; `write/tests.rs` deleted.
- `project_controller.rs` 8,846 → core + `rpc.rs`, tests co-located inline.
- `route.rs` 9,713 → 8 submodules (dispatch_only, authoritative_actor, pane_resolution, dispatch, session_resolution, cycle_ack, busy_pane, startup); `route/tests.rs` (11,619 lines, 268 tests) distributed inline + deleted.
- `preflight.rs` 6,609 → core + semantic_diff/run/maintenance; `preflight/tests.rs` (217 tests) distributed inline + deleted.
- `sync.rs` 6,265 → core + pane_repair/safe_passive/layout; `sync/tests.rs` distributed inline + deleted.
- `git.rs` 3,961 → core + dirs/normalize/safe_mutation; `git/tests.rs` (106 tests) distributed inline + deleted.
- `start.rs` 6,993 → core + run/supervisor_io/detection (alongside decisions/idle_watch); inline tests distributed.
- `template.rs` 6,009 (core) → core + tail_repair/prompt_comment_repair; inline tests distributed.
- `sim_world.rs` 5,093 → core + `engine.rs` (the 1,500-line `impl SimWorld` moved out, methods widened to pub(crate)).
- `session_check.rs` 5,225 → core + response_guards/detect; `session_check/tests.rs` distributed (helper-free guard tests → submodules; integration tests + helpers that shadow core fns stay in core test mod — see distribute_sc.py).

**COMPLETE as of 2026-06-12 (30 commits): every targeted large module decomposed; ZERO separate `<mod>/tests.rs` files remain in the workspace.** The biggest `.rs` files (session_check 396KB, sync 338KB, write/ipc 335KB) are large only because they bundle comprehensive co-located tests — their FUNCTIONALITY is decomposed and reasonable. Lower-tier files still undecomposed (inline tests, no separate file): `resync.rs`, `orchestrate.rs`, `main.rs` — optional further work if size matters.

**session_check GOTCHA (hit + reverted, not landed):** functionality extraction (response_guards 415-986, detect 685-1132; widen `ResponseSource::as_str` to pub(crate)) is clean. The `mod th` glob-ambiguity (E0659) was FIXED in distribute_multi2.py by re-exporting `pub(crate) use th::{explicit, names}` instead of `th::*`. BUT session_check has a deeper blocker: `session_check/tests.rs` defines TEST HELPERS that intentionally SHADOW core fn names — `fn inspect`, `fn inspect_with_warnings` (wrappers over core `pub fn inspect`/`inspect_with_warnings`). Hoisting these to module level / re-exporting collides (E0255 "defined multiple times"). Fix needed: only hoist helpers actually used by SUBMODULE-bound tests; leave core-only helpers (inspect etc.) inside session_check core's own test mod (don't re-export them). Requires per-helper usage analysis in the distributor. distribute_multi2.py (`/tmp/`) now handles const→pub(crate), skips `fn drop`, and explicit th re-export — extend it with helper-usage gating for session_check.

**Proven pipeline (per module):**
1. Extract a cohesive contiguous functionality cluster → `<mod>/<cluster>.rs` with `use super::*;` header; bump moved private items to `pub(crate)`; in parent add `mod <cluster>;` + re-export. Use `pub use` only if the cluster has ≥1 `pub` item, else `pub(crate) use` (clippy `unused-glob-reexport` under `-D warnings`).
2. **Boundary care:** end a cluster at the function's closing `}`, NOT including the next item's leading doc-comment — off-by-one cuts a `}` (unexpected-delimiter) or strands a `///` (cannot-find cascade). Verify with `sed` before extracting.
3. Relocate shared test helpers/statics to the **module level** as `#[cfg(test)] pub(crate)` (they call module-private fns, so they can't go to `test_support`); descendant test mods reach them via the re-export glob.
4. Classify each test by which submodule's symbols it references most; append inline `#[cfg(test)] mod tests { #![allow(unused_imports)] <prelude> ... }`. Scripts live in `/tmp/` (extract_block.py, distribute_generic.py, distribute_route.py).
5. Widen struct fields / under-private types to `pub(crate)` (clippy `private_interfaces`).
6. **FlowCore gate** (`tests/test_cli.rs::flowcore_hot_path_token_budget`): tracks `guard_`/`proof=`/`reason=`/`accepted_only` etc. per file. Moving code shifts counts — add each new submodule to `hot_paths` and update budgets to actuals (`grep -o tok file | wc -l`).
7. `make check` after each (RTK can mask exit — capture `$?` + grep `result: FAILED`).
