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

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/fb8de777e65629bf156ef66517a44d0d9c829c7e89b02b868f8ca9d0519d3ddc-20260528-224507.md`*

Compacted content:
- Archived 2 response topic(s): do #epv5; do #vb8h
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/fb8de777...

### Re: do #e130 — apply_patch FFI relocated; FFI surface complete — opus-4-7

Executed `do [#e130]` (`#k9e1-ffi-apply-patch`) — **the final FFI relocation**. The three `apply_patch` variants now live in `agent-doc-core::ffi`, and with them the entire pure-FFI surface is in core.

#### What landed

Submodule `3eeecd5f` on `20260422/codex-agent-backend`, pushed. Superproject `a8ff03528` on `main`, pushed.

**Relocated to `agent-doc-core/src/ffi.rs`:** `agent_doc_apply_patch`, `agent_doc_apply_patch_with_caret`, `agent_doc_apply_patch_with_boundary`.

**The `dummy_path` hack is gone.** Each function previously called the file-based `template::apply_patches_with_overrides(doc, …, Path::new("/dev/null"), …)`. They now call `apply_patches_with_overrides_pure` directly with `summary = None` and empty component/max_lines configs — the correct values for an editor FFI call that patches in-memory content with explicit mode overrides and no backing file. `FfiPatchResult` + helpers were already in core from `#epv5`. Removed the now-unused `use crate::{component, template};` from main; `force_link_core_ffi_symbols` references all three.

**Minor behavior note:** the pre-patch boundary id no longer carries a `:null` slug (it was derived from `/dev/null`'s file stem — an accidental artifact of the dummy path). No test depended on it; verified by repo-wide search.

#### Verification

- `cargo build` / `cargo build --release`: green.
- **`nm -D libagent_doc.so`**: 39 `agent_doc_` symbols export, including all 3 `apply_patch` variants — the full editor-plugin FFI surface resolves from the main cdylib unchanged.
- `cargo test --bin agent-doc`: **2,261 pass**; `--lib`: 96 pass (both unchanged).
- `cargo clippy -- -D warnings`: clean.
- Both binaries refreshed.

#### `#k9e1` FFI relocation — complete

All 15 pure FFI functions now live in `agent-doc-core::ffi` across the four sub-tasks (`#k9e1` POC + `#epv5` + `#vb8h` + `#e130`). The editor-plugin slim-link target — linking only `agent-doc-core` (9.87s cold, 74 crates) instead of main (129s, 266 crates) — is now **unblocked** for the document-mutation surface.

#### Recommendation on the tracker item

`#yybc` (`#k9e1-ffi-mem`) tracked the relocation pattern "for future pure FFI relocations" — and those are all done. The item has no remaining purpose; I'd strike it from the queue (it sits behind the publish/orchestration items) rather than spend a cycle on a no-op. Leaving that to your call since you own the queue ordering.

Plan: tasks/agent-doc/plan-agent-doc-core-extraction.md

**Not invoking `/loop`** — the next queue item is an operator decision (publish `agent-doc-core` to crates.io — SemVer commitment, ownership readiness) that I can't execute unilaterally.
<!-- agent:boundary:96d4ca76 -->
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
- ~do [#e130]~
- do [#77v6]
- do [#ysv9]
- do [#yybc]
- do [#adcr]
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog -->
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
