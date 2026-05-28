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

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/fb8de777e65629bf156ef66517a44d0d9c829c7e89b02b868f8ca9d0519d3ddc-20260528-001745.md`*

Compacted content:
- Archived 5 response topic(s): Clear Session Context error; Run Agent Doc dispatch proof timeout; do option 2; 2 more
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/fb8de777...
### Re: #n89s #xd6k bug fixes — glm-5.1

**#n89s** Fixed `dispatch_only_dispatch_start_proof_required` to return `false` for opencode harness. Updated test and downstream route assertion.

**#xd6k** Added `is_bare_model_name()` validator and opencode model name validation. `canonical_model_name` warns on bare names; `parse_model_arg` rejects them. Four new tests.

**#adcr** remains open — crate extraction needs dedicated planning.

**Verification:** `make check` 2514+470 tests green. Pushed as `e3e9d9a0`.
What are #next-steps for [#adcr]

### Re: #next-steps for #adcr — opus-4-7

The #adcr umbrella stays at top of backlog. Concrete next steps, added in priority order below it:

1. **#adcr-dep-audit** — survey which of the 7 listed modules ({template, diff, component, frontmatter, crdt, pending, model_tier}) import from orchestration code (preflight, plan, cycle_state, dispatch, route, ipc, git, harness). Tools: `cargo modules` or `tsift graph <module>::* --callers`. Output: dependency table separating leaves from coupled modules. This is a precondition — extracting a module that pulls in `git.rs` or `route.rs` defeats the fast-compile goal.

2. **#adcr-api-surface** — define what `agent-doc-core` exports. Pure data types only: `DocumentTemplate`, `Component`, `Frontmatter`, `Diff` patches, `Crdt` ops, `Pending` model, `ModelTier` resolution. Anything that touches IPC, git, tmux, or harness probing stays in the main crate. Document the surface as a `pub use` block in a draft `agent-doc-core/src/lib.rs`.

3. **#adcr-workspace** — convert `src/agent-doc/` into a Cargo workspace root with `agent-doc-core` as a member. Capture compile-time baseline (`cargo build -p agent-doc-core --timings`) before extraction so the FFI/editor-plugin win is measurable.

After those three land, extraction proceeds module-by-module starting with leaves (likely `model_tier` and `frontmatter` based on file size and naming; confirmed by step 1's audit). The existing `plan-agent-doc-flowcore-refactor.md` is complementary, not duplicate — that plan targets orchestration use-case flows, this one targets the data/orchestration crate seam.

**Top of backlog:** #adcr
<!-- agent:boundary:3664c6f4 -->
<!-- /agent:exchange -->

## Queue

<!-- agent:queue -->
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog -->
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
<!-- /agent:done -->
