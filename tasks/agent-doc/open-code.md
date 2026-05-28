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

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/fb8de777e65629bf156ef66517a44d0d9c829c7e89b02b868f8ca9d0519d3ddc-20260528-221143.md`*

Compacted content:
- Archived 12 response topic(s): #next-steps to split out crates; do #k5s7; do #n7dd; 9 more
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/fb8de777...

### Re: do #epv5 — 4 simple pure FFI functions relocated to core — opus-4-7

Executed `do [#epv5]` (`#k9e1-ffi-simple`). The four simplest pure FFI functions now live in `agent-doc-core::ffi`, following the `#k9e1` POC pattern. Symbols verified still exported from the main cdylib.

#### What landed

Submodule `d74212ae` on `20260422/codex-agent-backend`, pushed. Superproject `8c5aead65` on `main`, pushed.

**Relocated to `agent-doc-core/src/ffi.rs`:**
- `agent_doc_parse_components` (+ `FfiComponentList`)
- `agent_doc_visual_tokens_json`
- `agent_doc_merge_frontmatter`
- `agent_doc_normalize_template_structure`

**Foundational enabling moves (necessary — not scope creep):**
- **`FfiPatchResult` + `ffi_patch_ok`/`ffi_patch_err`/`ffi_patch_from_result` + `normalize_editor_visible_result` moved to core.** Two of the four functions return the shared `FfiPatchResult`, so the type had to relocate before they could. The apply_patch and `reposition_boundary_*` families still in main now resolve it via the existing `pub use agent_doc_core::ffi::*` re-export. This also **front-loads the shared-type move that `#e130` was scheduled to do** — I edited `#e130`'s backlog text to reflect that only the 3 apply_patch bodies remain.
- **`utf8_offsets_to_utf16_offsets`** helper moved to core (`visual_tokens_json` needs it).
- **`syntax` module relocated** `src/syntax.rs` → `agent-doc-core/src/syntax.rs`. It's pure (`serde` + `component::find_code_ranges`, both in core) and its only consumer was the relocated `visual_tokens_json`. Removed from main's `lib.rs`.

`force_link_core_ffi_symbols` extended to reference all 4 new `extern "C"` fns so the static linker keeps them in the cdylib export table.

#### Verification

- `cargo build` / `cargo build --release`: green.
- **`nm -D target/release/libagent_doc.so`**: all 6 symbols present (`agent_doc_free_string`, `agent_doc_free_state` from the POC + the 4 relocated). Editor plugins (JetBrains/VS Code/Python) resolve them unchanged.
- `cargo test -p agent-doc-core --lib`: syntax module 7 pass.
- `cargo test --lib` (main): 96 pass. `cargo test --bin agent-doc`: **2,261 pass** (unchanged — main's ffi test module exercises the re-exported symbols end-to-end, confirming the relocation is transparent).
- `cargo clippy -- -D warnings`: clean.
- `make check`: only the pre-existing `codex_bare_run_inside_owning_pane_fails_before_nested_dispatch` failure (environmental — needs live tmux/codex; reproduces on pristine HEAD; GitHub CI green). Unrelated to FFI relocation.
- `cargo install --path .` + `cargo build --release`: both binaries refreshed.

#### Remaining FFI relocation (queue order)

- `#vb8h` (`#k9e1-ffi-boundary`) — 4 reposition_boundary variants + 2 CRDT FFI fns + `FfiMergeResult`. `FfiPatchResult` already in core, so these are now pure body moves.
- `#e130` (`#k9e1-ffi-apply-patch`) — reduced to the 3 apply_patch bodies (shared type + helpers already moved this cycle).

Plan: tasks/agent-doc/plan-agent-doc-core-extraction.md

**Not invoking `/loop`** — queue head advances to `do [#vb8h]`. Each FFI sub-task warrants its own review/symbol-verification cycle; leaving the queue advance to the operator.

### Re: do #vb8h — boundary + CRDT FFI relocated to core — opus-4-7

Executed `do [#vb8h]` (`#k9e1-ffi-boundary`). The CRDT and reposition-boundary FFI functions now live in `agent-doc-core::ffi`. Pure body moves — the shared `FfiPatchResult` + helpers were already relocated in `#epv5`, so this cycle was clean lifting.

#### What landed

Submodule `ae48c394` on `20260422/codex-agent-backend`, pushed. Superproject `fb9f7ccdc` on `main`, pushed.

**Relocated to `agent-doc-core/src/ffi.rs`:**
- `agent_doc_crdt_merge` + `agent_doc_merge_crdt` (+ `FfiMergeResult` struct)
- `agent_doc_reposition_boundary_to_end` (+ `_with_id`, `_preserve_head`, `_preserve_head_with_id`)

All depend only on `agent_doc_core::{crdt, template}` plus the already-in-core `FfiPatchResult`/helpers. Removed the now-unused `use crate::crdt;` from main's `ffi.rs`. `force_link_core_ffi_symbols` extended to reference all 6 new `extern "C"` fns.

#### Verification

- `cargo build` / `cargo build --release`: green.
- **`nm -D libagent_doc.so`**: all **12** relocated symbols present in the export table (6 this cycle + 4 from `#epv5` + 2 POC). Editor plugins resolve them unchanged.
- `cargo test --bin agent-doc`: **2,261 pass**; `--lib`: 96 pass (both unchanged — main's ffi test module exercises the re-exported symbols).
- `cargo clippy -- -D warnings`: clean.
- Both binaries refreshed (`cargo install --path .` + `cargo build --release`).

#### FFI relocation status

Only `#e130` (`#k9e1-ffi-apply-patch`) remains — and it's now reduced to the 3 `apply_patch` bodies (the shared type + helpers moved in `#epv5`). Each still needs `template::apply_patches_with_overrides` switched to `apply_patches_with_overrides_pure` (preloaded empty configs, dropping the `dummy_path`). After `#e130`, the full pure-FFI surface lives in core and the editor-plugin slim-link target is unblocked.

Plan: tasks/agent-doc/plan-agent-doc-core-extraction.md

**Not invoking `/loop`** — queue head advances to `do [#e130]`; the apply_patch group touches the `#ckv3`-refactored signature and warrants its own review + symbol-verification cycle.
<!-- agent:boundary:94e44c5b -->
<!-- /agent:exchange -->

## Queue

<!-- agent:queue auto -->
preset #spec-test-build-install-commit-push
- ~do [#k5s7]~
- ~do [#n7dd]~
- ~do [#0ejf]~
- ~do [#ckv3]~
- ~do [#0c4e]~
- ~do [#bjrv]~
- ~do [#k9e1]~
- ~do [#5yqz]~
- ~do [#epv5]~
- ~do [#vb8h]~
- do [#e130]
- do [#77v6]
- do [#ysv9]
- do [#yybc]
- do [#adcr]
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog -->
- [ ] [#e130] #k9e1-ffi-apply-patch Move the 3 apply_patch variants (agent_doc_apply_patch, _with_caret, _with_boundary) to agent-doc-core::ffi. NOTE: FfiPatchResult + ffi_patch_ok/err/from_result + normalize_editor_visible_result were ALREADY relocated to core in #epv5 — only the 3 function bodies remain. Each needs template::apply_patches_with_overrides switched to apply_patches_with_overrides_pure (preloaded empty configs, no dummy_path). Touches the #ckv3-refactored patch surface.
- [ ] [#77v6] #adcr-publish Decide on publishing agent-doc-core to crates.io (currently publish = false). Allows third-party FFI consumers and unblocks the editor-plugin slim-link target.
- [ ] [#ysv9] #adoc-orchestration-crate Plan agent-doc-orchestration extraction — split route/git/sessions/ipc_socket/supervisor/capture/cycle_state/repair/sync/start out of the main crate into a sibling workspace member, leaving only the CLI entrypoint + command dispatch in agent-doc. Would create a 3-layer architecture (CLI → orchestration → core) and substantially cut the editor-plugin-only build target. Substantial refactor; depends on #adcr-baseline-measure to quantify the win first.
- [ ] [#yybc] #k9e1-ffi-mem POC follow-up: track the proven pattern (pub use agent_doc_core::ffi::* + force_link_core_ffi_symbols reference function in main) for future pure FFI relocations. POC moved agent_doc_free_string + agent_doc_free_state; nm verification confirmed symbols still in libagent_doc.so export table.
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
