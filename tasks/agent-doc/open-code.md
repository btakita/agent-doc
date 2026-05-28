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
queue_active: false
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
<!-- agent:boundary:e09684a2 -->
<!-- /agent:exchange -->

## Queue

<!-- agent:queue -->
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog -->
- [ ] [#pjb4] #adcr-wave-1 Extract leaf modules into agent-doc-core (no seam decisions, ~7.6k LOC) — relocate new_boundary_id/new_boundary_id_with_summary/format_boundary_marker/BOUNDARY_ID_LEN from lib.rs to agent_doc_core::id; move crdt.rs (CrdtDoc, merge, dedup_adjacent_blocks, compact); move model_tier.rs (Tier, TierMap, ModelConfig, ModelSwitchScan + pure resolvers; keep detect_harness wrapper in main crate); move pending.rs (PendingItem, PendingState, PendingOp + all op_* functions); move component.rs (Component, parse, strip_comments, find_*ranges, parse_attrs, BACKLOG_COMPONENT and friends). Capture cold-from-scratch baselines via cargo clean + cargo build -p agent-doc-core --release --timings before extraction; update agent-doc-core/COMPILE_BASELINE.md with the Wave 1 row. Plan: tasks/agent-doc/plan-agent-doc-core-extraction.md
- [ ] [#yhsr] #adcr-wave-2 Extract frontmatter.rs + project_config.rs together (mutual refs) into agent-doc-core — Frontmatter, AgentDocFormat, AgentDocWrite, CollaborationMode, CodexNetworkAccess, ResolvedMode, StreamConfig, PendingCaptureGuardMode, LintDialectMode, ProjectConfig, GuardConfig, LintConfig, SshConfig/SshDocConfig/SshProfileConfig, ComponentConfig + pure parse/write/merge_fields/set_*. Keep ensure_session_for_file/set_format_and_write/read_session_id/load_project*/save_project/*_tmux_session wrappers in main crate. Depends on Wave 1 (model_tier). Plan: tasks/agent-doc/plan-agent-doc-core-extraction.md
- [ ] [#yxvq] #adcr-wave-3 Extract template.rs (~5.7k LOC) into agent-doc-core::template — PatchBlock, TemplateInfo, ComponentInfo, parse_patches, apply_patches, apply_patches_with_overrides, all repair_* functions, normalize_editor_visible_template_structure, all reposition_boundary_* helpers, is_template_mode, exchange_baseline_headings. Keep template_info(&Path) wrapper in main crate. Depends on Wave 2 (frontmatter/project_config) + Wave 1 (component). Plan: tasks/agent-doc/plan-agent-doc-core-extraction.md
- [ ] [#mzzd] #adcr-wave-4 Extract diff.rs into agent-doc-core per #rtx6 Option 1 — rename current src/agent-doc/src/diff.rs to diff_io.rs (strip to compute/compute_with_current/wait_for_stable_content/is_stale_snapshot/run, keep snapshot::{load,save,resolve,path_for} calls); move pure half (DiffType, DiffClassification, PromptBearingChange, ParsedSlashCommands, OrchestrationRequest, strip_comments, annotate_diff, classify_diff, slash command + orchestration detection, prompt-prefix normalization, prompt-bearing-change classification, unified_diff_from_contents) to agent_doc_core::diff; re-export at agent_doc::diff so the 6 production call sites compile unchanged. Plan: tasks/agent-doc/plan-agent-doc-core-extraction.md
- [ ] [#adcr] Extract agent-doc-core crate (~17K lines) — enables fast-compiling FFI/editor-plugin target independent of orchestration layer
<!-- /agent:backlog -->

## Icebox

<!-- agent:icebox -->
<!-- /agent:icebox -->

## Completed / Reaped

<!-- agent:done -->
- 2026-05-27 [#fr8q] [recommended] Compact this session (116 exchange lines, block-level accretion) before next implementation turn
- 2026-05-27 [#n89s] #agent-doc-bug Fix `dispatch_only_dispatch_start_proof_required` to return `false` for opencode harness (routed_reopen.rs:319) — interim fix until tracker is built
- 2026-05-27 [#xd6k] #agent-doc-bug Add model name validation in `canonical_model_name` or probe builder for opencode harness — reject bare names without provider prefix (model_tier.rs:256, codex.rs:534)
- 2026-05-27 [#adcr] Extract agent-doc-core crate (~17K lines: template, diff, component, frontmatter, crdt, pending, model_tier + leaf modules) — enables fast-compiling FFI/editor-plugin target independent of orchestration layer
- 2026-05-28 [#stqc] #adcr-dep-audit Survey inbound deps from orchestration code (preflight/plan/cycle_state/dispatch/route/ipc/git/harness) into {template, diff, component, frontmatter, crdt, pending, model_tier}; identify leaf vs coupled modules — precondition for #adcr
- 2026-05-28 [#5x1r] #adcr-api-surface Define agent-doc-core export surface (pure types: DocumentTemplate, Component, Frontmatter, Diff patches, Crdt ops, Pending model, ModelTier resolution) — keep orchestration types out
- 2026-05-28 [#rtx6] [recommended] #adcr-snapshot-seam Decide diff.rs ↔ snapshot.rs seam — either extract Snapshot data type into agent-doc-core and keep load/save/resolve/path_for in orchestration, or inject snapshot through a trait. Required before extracting diff.
- 2026-05-28 [#d97v] #adcr-workspace Convert src/agent-doc into Cargo workspace root; add agent-doc-core stub crate; record cargo build --timings baselines per tasks/agent-doc/plan-agent-doc-core-extraction.md
<!-- /agent:done -->
