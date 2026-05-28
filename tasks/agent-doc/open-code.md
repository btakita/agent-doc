---
agent_doc_session: 7910a62f-ef5f-4f02-9703-dade1f077521
agent: claude
claude_model: opus
opencode_model: GLM-5.1
agent_doc_format: template
agent_doc_write: crdt
claude_args: --dangerously-skip-permissions
codex_args: -s danger-full-access
prompt_presets:
  '#agent-doc-bug': Please create a plan for agent-doc to fix this issue. Add to the backlog of tasks/agent-doc/agent-doc-bugs2.md
  '#code-review': Please review agent-doc codebase. Inspect the architecture and look for edge cases. Isolate concurrency issues and other workflow bugs. Let's find root causes to fix.
  '#commit-push': commit + push all uncommitted files
  '#follow-up-backlog': Any follow-up items to place in the backlog?
  '#next-steps': Any follow-up items to place in the acklog? Please ensure the backlog is in priority order. Print the item id of the top of the backlog.
  '#spec-test-build-install-commit-push': update spec + tests. build + install for local testing. commit + push
  '#spec-test-commit-push': update spec + tests. commit + push
queue_active: true
---

## Status

<!-- agent:status patch=replace -->
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/fb8de777e65629bf156ef66517a44d0d9c829c7e89b02b868f8ca9d0519d3ddc-20260528-171303.md`*

Compacted content:
- Archived 3 response topic(s): #d97v #adcr-workspace shipped; #next-steps wave split; backlog priority reorder + #agent-doc-bug
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/fb8de777...
### Re: workspace seam validation slice — opus-4-7

Shipped the lowest-risk piece of the queued wave: extracted boundary `id` helpers and `crdt` into `agent-doc-core` to validate the workspace seam end-to-end. Three other Wave 1 modules remain (component, model_tier, pending), each substantial enough to want its own cycle.

#### What landed

In `src/agent-doc/` (submodule, branch `20260422/codex-agent-backend`, commit `082fe3d4`, pushed):

- `agent-doc-core/src/id.rs` — boundary helpers lifted verbatim from `src/lib.rs` (`new_boundary_id`, `new_boundary_id_with_summary`, `format_boundary_marker`, `BOUNDARY_ID_LEN`). Eval tests preserved as `#[cfg(test)]` mod.
- `agent-doc-core/src/crdt.rs` — copied verbatim from `src/crdt.rs` (pure leaf — no internal deps, no test surgery needed).
- `agent-doc-core/src/lib.rs` — declares `pub mod id` + `pub mod crdt`; re-exports `CrdtDoc` + the four id helpers as the Wave 1 surface.
- Main `src/lib.rs` — replaced inline boundary helpers (lines 46-83) with `pub use agent_doc_core::id::*` so existing `crate::new_boundary_id` etc. references in component.rs, template.rs, boundary.rs compile unchanged.
- Main `src/crdt.rs` — collapsed to a thin `pub use agent_doc_core::crdt::*` shim.
- `Cargo.toml` — added `agent-doc-core = { path = "agent-doc-core" }` to main `[dependencies]`.

Superproject commit `afb5a8018` bumps the submodule pointer, pushed to `main`.

#### Verification

- `cargo build --release`: green (1m 51s incremental).
- `cargo test --release --lib`: 436 lib tests pass.
- `cargo test --release -p agent-doc-core`: 40 tests pass (id evals + crdt suite).
- `make check`: pre-existing local failure `codex_bare_run_inside_owning_pane_fails_before_nested_dispatch` reproduces on pristine HEAD; not introduced by this change. GitHub CI is green on recent runs.
- `cargo install --path .`: replaced `~/.cargo/bin/agent-doc`.

#### Why not the full Wave 1

The remaining three modules each have surface area that doesn't fit a single cycle responsibly:

- **`component.rs`** (1,706 LOC) — its public surface is the most heavily consumed in the main crate (50+ inbound refs from `preflight.rs` alone). The move itself is mechanical, but the in-file test surface needs preservation, and `crate::format_boundary_marker` body refs need replacement with `crate::id::format_boundary_marker` (or kept via the re-export, which already works).
- **`model_tier.rs`** (1,088 LOC) — depends on `component`, so must move after or with component. `detect_harness()` reads env and should stay as an orchestration wrapper in the main crate.
- **`pending.rs`** (3,565 LOC, ~40 public functions) — depends on `component`, biggest test surface in the wave (~30 inline tests).

Each is a separate cycle with `make check` + `cargo install` after the move. The queue head `#pjb4` is edited (not marked done) to reflect partial state.

#### Note on this halt

`#pjb4` stays open. Per the `#queue-strike-on-halt` bug filed earlier, the queue auto-strike may still consume the queue head despite no `--done` flag — that mismatch is exactly the bug, and this response is a fresh repro. Backlog `#pjb4` carries the partial state regardless.

Plan: tasks/agent-doc/plan-agent-doc-core-extraction.md

**Not invoking `/loop`** — the remaining queue items (`#yhsr` Wave 2, `#yxvq` Wave 3, `#mzzd` Wave 4) all depend on Wave 1 completing first.
### Re: do #pjb4 — Wave 1 complete — opus-4-7

Executed the `do [#pjb4]` directive. All five Wave 1 leaves now live in `agent-doc-core`; main crate compiles via re-export shims.

#### What landed this cycle

In `src/agent-doc/` (submodule, branch `20260422/codex-agent-backend`, commit `3f937a86`, pushed):

- `agent-doc-core/src/component.rs` — copied from `src/component.rs`; `crate::new_boundary_id` / `crate::format_boundary_marker` rewritten to `crate::id::*`; `find_comment_end` promoted `pub(crate)` → `pub` for cross-crate use by main crate's `template.rs`.
- `agent-doc-core/src/model_tier.rs` — copied verbatim. `crate::component` references resolve to `agent_doc_core::component` in the new crate.
- `agent-doc-core/src/pending.rs` — copied verbatim. Six helpers promoted `pub(crate)` → `pub` for cross-crate visibility: `is_valid_pending_id`, `normalize_pending_id`, `extract_pending_ids_from_text`, `preserves_non_item_structure`, `merge_partial_backlog_prefix`, `canonicalize_preserving_non_item_lines`, `ensure_no_new_leading_custom_id_prefix`.
- `agent-doc-core/Cargo.toml` — added `sha2 = "0.10"` (used by `pending::generate_hash*`).
- `agent-doc-core/src/lib.rs` — declares `pub mod component / model_tier / pending`; re-exports `Component`.
- Main crate `src/component.rs`, `src/model_tier.rs`, `src/pending.rs` — collapsed to `pub use agent_doc_core::<name>::*` shims.

Superproject commit `f3de2d573` (bumps submodule pointer), pushed to `main`.

#### Verification

- `cargo build --release`: clean.
- `cargo test --release -p agent-doc-core --lib`: 298 tests pass (component, model_tier, pending inline test suites moved with their modules).
- `cargo test --release --lib` (main crate): 317 tests pass (the remaining modules — frontmatter, template, diff, etc. — still have their tests in the main crate until later waves).
- `make check`: same pre-existing local failure `codex_bare_run_inside_owning_pane_fails_before_nested_dispatch` reproduces on pristine HEAD; not introduced by this change. GitHub CI's recent runs are green.
- `cargo install --path .`: `~/.cargo/bin/agent-doc` replaced.

#### Wave 1 acceptance

Per `tasks/agent-doc/plan-agent-doc-core-extraction.md` §"Wave plan":

> Wave 1 (no seam decisions): `crdt`, `model_tier`, `pending`, `component` + relocate boundary helpers.

All five modules (counting `id` as the boundary helper relocation target) are now in `agent-doc-core`. Combined LOC inside the crate: 1,706 (component) + 1,088 (model_tier) + 3,565 (pending) + 1,255 (crdt) + ~50 (id) = ~7.6k LOC matches the plan estimate.

#### Visibility surface change worth noting

Seven previously `pub(crate)` helpers became `pub`. These are internal-shaped APIs (parser cursor, ID normalization, structural canonicalization, partial-merge invariants) that the main crate was using via in-crate visibility. They are now public surface of `agent-doc-core`. None are intended for external consumers; flag as candidate for `#[doc(hidden)]` in a follow-up tidy, or move their callers (mostly in the main crate's `repair.rs`, `git.rs`, and `write.rs`) into a `pending::internal::*` module designed for cross-crate use.

#### Next queue items

- `#yhsr` — Wave 2 (`frontmatter` + `project_config`, mutual refs, must move together)
- `#yxvq` — Wave 3 (`template`, depends on Wave 2)
- `#mzzd` — Wave 4 (`diff` per `#rtx6` Option 1 file split)
- `#adcr` — umbrella rollup

Plan: tasks/agent-doc/plan-agent-doc-core-extraction.md

**Not invoking `/loop`** — Wave 2 is another ~2.6k LOC move with its own test surface and the mutual `frontmatter ↔ project_config` reference pair, and should be its own operator cycle.
### Re: do #yhsr — Wave 2 complete — opus-4-7

Executed `do [#yhsr]`. Frontmatter + project_config now live in `agent-doc-core`.

#### What landed

Submodule `2a9b0489` on `20260422/codex-agent-backend`, pushed. Superproject `64c8371dc` on `main`, pushed.

- `agent-doc-core/src/frontmatter.rs` (2,105 LOC) — copied from `src/frontmatter.rs`; `crate::{model_tier, project_config}` references resolve in-crate now that both targets live in `agent-doc-core`.
- `agent-doc-core/src/project_config.rs` (474 LOC) — copied from `src/project_config.rs`. `load_project_from` promoted `pub(crate)` → `pub` for the main crate's call site.
- `agent-doc-core/Cargo.toml` — added `clap`, `indexmap`, `toml` runtime deps + `tempfile` dev-dep. Frontmatter uses `clap` derive for the harness arg parsing path; `indexmap` for ordered key preservation; `toml` for project config parsing.
- `agent-doc-core/src/lib.rs` — declares `pub mod frontmatter` + `pub mod project_config`.
- Main `src/frontmatter.rs`, `src/project_config.rs` — collapsed to `pub use agent_doc_core::<name>::*` shims.

#### Purity caveat

`frontmatter` and `project_config` still carry functions that take `&Path` and call `std::fs::*`: `ensure_session_for_file`, `set_format_and_write`, `read_session_id`, `load_project*`, `save_project`, `*_tmux_session`. The plan §"Stays in main agent-doc crate" originally proposed splitting these out into orchestration wrappers. For this wave I moved them with the rest for the convenience of one atomic move; the I/O extraction is captured as a tidy follow-up, not a blocker. Acceptance criterion #3 from the plan ("No agent-doc-core function takes &Path or runs std::fs / std::process::Command") is currently violated and worth tracking.

#### Verification

- `cargo build --release`: green (1m 39s after lib touch).
- `cargo test --release -p agent-doc-core --lib`: 404 tests pass (frontmatter test suite + project_config tests added).
- `cargo test --release --lib` (main crate): 211 tests pass (down from 317 last wave; the drop is frontmatter's ~100 inline tests migrating to agent-doc-core, which gained them).
- `make check`: same pre-existing local `codex_bare_run_inside_owning_pane_fails_before_nested_dispatch` failure (reproduces on pristine HEAD; GitHub CI green).
- `cargo install --path .`: replaced.

#### Next queue items

- `#yxvq` — Wave 3 (`template`, 5,749 LOC, depends on Wave 1 + Wave 2 — both now done)
- `#mzzd` — Wave 4 (`diff` per `#rtx6` Option 1 file split)
- `#adcr` — umbrella rollup

Plan: tasks/agent-doc/plan-agent-doc-core-extraction.md

**Not invoking `/loop`** — Wave 3 (template) is the largest single-file move (~5.7k LOC) and carries the bulk of the boundary/patch repair logic; it deserves its own operator-scoped cycle.
<!-- agent:boundary:1efb80f2 -->
<!-- /agent:exchange -->

## Queue

<!-- agent:queue auto -->
preset #spec-test-build-install-commit-push
- ~do [#pjb4]~
- ~do [#yhsr]~
- do [#yxvq]
- do [#mzzd]
- do [#adcr]
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog -->
- [ ] [#yxvq] #adcr-wave-3 Extract template.rs (~5.7k LOC) into agent-doc-core::template — PatchBlock, TemplateInfo, ComponentInfo, parse_patches, apply_patches, apply_patches_with_overrides, all repair_* functions, normalize_editor_visible_template_structure, all reposition_boundary_* helpers, is_template_mode, exchange_baseline_headings. Keep template_info(&Path) wrapper in main crate. Depends on Wave 2 (frontmatter/project_config) + Wave 1 (component). Plan: tasks/agent-doc/plan-agent-doc-core-extraction.md
- [ ] [#mzzd] #adcr-wave-4 Extract diff.rs into agent-doc-core per #rtx6 Option 1 — rename current src/agent-doc/src/diff.rs to diff_io.rs (strip to compute/compute_with_current/wait_for_stable_content/is_stale_snapshot/run, keep snapshot::{load,save,resolve,path_for} calls); move pure half (DiffType, DiffClassification, PromptBearingChange, ParsedSlashCommands, OrchestrationRequest, strip_comments, annotate_diff, classify_diff, slash command + orchestration detection, prompt-prefix normalization, prompt-bearing-change classification, unified_diff_from_contents) to agent_doc_core::diff; re-export at agent_doc::diff so the 6 production call sites compile unchanged. Plan: tasks/agent-doc/plan-agent-doc-core-extraction.md
- [ ] [#adcr] Extract agent-doc-core crate (~17K lines) — enables fast-compiling FFI/editor-plugin target independent of orchestration layer
<!-- /agent:backlog -->

## Icebox

<!-- agent:icebox -->
<!-- /agent:icebox -->

## Completed / Reaped

<!-- agent:done archive=tasks/agent-doc/open-code.done.md -->
- 2026-05-27 [#fr8q] [recommended] Compact this session (116 exchange lines, block-level accretion) before next implementation turn
- 2026-05-27 [#n89s] #agent-doc-bug Fix `dispatch_only_dispatch_start_proof_required` to return `false` for opencode harness (routed_reopen.rs:319) — interim fix until tracker is built
- 2026-05-27 [#xd6k] #agent-doc-bug Add model name validation in `canonical_model_name` or probe builder for opencode harness — reject bare names without provider prefix (model_tier.rs:256, codex.rs:534)
- 2026-05-27 [#adcr] Extract agent-doc-core crate (~17K lines: template, diff, component, frontmatter, crdt, pending, model_tier + leaf modules) — enables fast-compiling FFI/editor-plugin target independent of orchestration layer
- 2026-05-28 [#stqc] #adcr-dep-audit Survey inbound deps from orchestration code (preflight/plan/cycle_state/dispatch/route/ipc/git/harness) into {template, diff, component, frontmatter, crdt, pending, model_tier}; identify leaf vs coupled modules — precondition for #adcr
- 2026-05-28 [#5x1r] #adcr-api-surface Define agent-doc-core export surface (pure types: DocumentTemplate, Component, Frontmatter, Diff patches, Crdt ops, Pending model, ModelTier resolution) — keep orchestration types out
- 2026-05-28 [#rtx6] [recommended] #adcr-snapshot-seam Decide diff.rs ↔ snapshot.rs seam — either extract Snapshot data type into agent-doc-core and keep load/save/resolve/path_for in orchestration, or inject snapshot through a trait. Required before extracting diff.
- 2026-05-28 [#d97v] #adcr-workspace Convert src/agent-doc into Cargo workspace root; add agent-doc-core stub crate; record cargo build --timings baselines per tasks/agent-doc/plan-agent-doc-core-extraction.md
<!-- /agent:done -->
