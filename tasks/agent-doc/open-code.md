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
<!-- agent:boundary:2548bae3 -->
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
- do [#epv5]
- do [#vb8h]
- do [#e130]
- do [#77v6]
- do [#ysv9]
- do [#yybc]
- do [#adcr]
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog -->
- [ ] [#epv5] #k9e1-ffi-simple Move the 4 simplest dependency-free pure FFI functions to agent-doc-core::ffi: agent_doc_parse_components (+ FfiComponentList), agent_doc_visual_tokens_json, agent_doc_merge_frontmatter, agent_doc_normalize_template_structure. Each touches only component::parse, agent_doc_core::frontmatter::merge_fields, or agent_doc_core::template::normalize_editor_visible_template_structure. Pattern is the POC + ~50 LOC per function. Update force_link_core_ffi_symbols to reference each.
- [ ] [#vb8h] #k9e1-ffi-boundary Move the 4 reposition_boundary_to_end variants (+ _with_id, _preserve_head, _preserve_head_with_id) plus agent_doc_crdt_merge + agent_doc_merge_crdt + their FfiMergeResult helper struct to agent-doc-core::ffi. All pure; depend on agent_doc_core::template and agent_doc_core::crdt.
- [ ] [#e130] #k9e1-ffi-apply-patch Move the 3 apply_patch variants (agent_doc_apply_patch, agent_doc_apply_patch_with_caret, agent_doc_apply_patch_with_boundary) plus FfiPatchResult and helpers (ffi_patch_ok/err/from_result, normalize_editor_visible_result). Each needs the template::apply_patches_with_overrides call switched to apply_patches_with_overrides_pure (preloaded empty configs, no dummy_path). Largest sub-piece — touches the recently-refactored patch surface from #ckv3.
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
