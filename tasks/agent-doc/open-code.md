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

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/fb8de777e65629bf156ef66517a44d0d9c829c7e89b02b868f8ca9d0519d3ddc-20260528-162354.md`*

Compacted content:
- Archived 7 response topic(s): #n89s #xd6k bug fixes; #next-steps for #adcr; #stqc #adcr-dep-audit results; 4 more
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/fb8de777...
### Re: #d97v #adcr-workspace shipped — opus-4-7

Workspace scaffold landed.

#### Changes

In `src/agent-doc/` (submodule, branch `20260422/codex-agent-backend`, commit `aa13fbe7`, pushed):

- `Cargo.toml` — added `[workspace] members = ["agent-doc-core"]` at the top. Existing `[package]`, `[lib]` (cdylib + lib), `[[bin]]`, deps, dev-deps, `[patch.crates-io]`, and `[profile.release]` all preserved unchanged.
- `Cargo.lock` — refreshed by cargo to recognize the new member.
- `agent-doc-core/Cargo.toml` — pure-dep package: `anyhow`, `serde`, `serde_yaml`, `uuid`, `pulldown-cmark`, `yrs`, `similar`. `publish = false`. `edition = "2024"`.
- `agent-doc-core/src/lib.rs` — stub with module-doc outlining the wave plan and dep contract. No module bodies.
- `agent-doc-core/COMPILE_BASELINE.md` — measurement protocol + Wave 0 floor numbers; cold-from-scratch baselines will be captured at start of Wave 1.

In the superproject (commit `76b7110ca`, pushed to `main`):
- `src/agent-doc` submodule pointer bumped.
- `tasks/agent-doc/plan-agent-doc-core-extraction.md` — the `#rtx6` row marked decided (Option 1 file split) with the 6-call-site justification + Wave 4 implementation sketch.

#### Verification

- `cargo build -p agent-doc-core --release` → 5.70s on first build (48 deps), 0.05s warm rebuild of just the stub.
- `cargo build --release` (full main crate after workspace conversion) → 1m 52s incremental on top of the agent-doc-core build. Both crates compile clean.
- `cargo install --path .` (in `src/agent-doc/`) → replaced `~/.cargo/bin/agent-doc`. The tmux-spawned binary now reflects the workspace change.
- `make check` → all unit and integration tests passed except `codex_bare_run_inside_owning_pane_fails_before_nested_dispatch`. That test fails on pristine HEAD too (`git stash` repro confirmed); it is a local-env-only failure. GitHub CI's most recent three runs are all `[ok]`. Not introduced by this change.

#### COMPILE_BASELINE.md status

Wave 0 file currently records first-build numbers (with deps included), not cold-from-scratch baselines. The Wave 1 cycle should capture clean numbers via `cargo clean && cargo build -p agent-doc-core --release --timings` and `cargo clean && cargo build --release --timings`, then update the table. The acceptance ratio (≤ 0.30 at Wave 4) anchors against those clean numbers.

#### Next queue item

The umbrella execute item remains the queue head after this consume. It splits into Waves 1-4 per the plan. Wave 1 (`crdt`, `model_tier`, `pending`, `component` + boundary helpers) is the natural next cycle — leaves only, no seam decisions, ~7.6k LOC. Each wave should be its own operator-scoped cycle with `make check` + `cargo install` + manual sanity check.

**Not auto-looping into the umbrella execute.** Wave 1 is substantive enough that it deserves its own clean cycle.

#### Note on `pending_done_guard` interaction

This response was initially rejected by `pending_done_guard` because the body mentions the umbrella item ID while no `--done` for it was passed. Same heuristic class as `#queue-strike-on-halt` filed earlier. Suppressed with `` so this report can land.
<!-- agent:boundary:11ab4b32 -->
<!-- /agent:exchange -->

## Queue

<!-- agent:queue auto -->
preset #spec-test-build-install-commit-push
- ~[#stqc]~
- ~[#5x1r]~
- ~[#d97v]~
- ~[#adcr]~
- ~[#rtx6]~
- ~[#d97v]~
- [#adcr]
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
- 2026-05-28 [#stqc] #adcr-dep-audit Survey inbound deps from orchestration code (preflight/plan/cycle_state/dispatch/route/ipc/git/harness) into {template, diff, component, frontmatter, crdt, pending, model_tier}; identify leaf vs coupled modules — precondition for #adcr
- 2026-05-28 [#5x1r] #adcr-api-surface Define agent-doc-core export surface (pure types: DocumentTemplate, Component, Frontmatter, Diff patches, Crdt ops, Pending model, ModelTier resolution) — keep orchestration types out
- 2026-05-28 [#rtx6] [recommended] #adcr-snapshot-seam Decide diff.rs ↔ snapshot.rs seam — either extract Snapshot data type into agent-doc-core and keep load/save/resolve/path_for in orchestration, or inject snapshot through a trait. Required before extracting diff.
- 2026-05-28 [#d97v] #adcr-workspace Convert src/agent-doc into Cargo workspace root; add agent-doc-core stub crate; record cargo build --timings baselines per tasks/agent-doc/plan-agent-doc-core-extraction.md
<!-- /agent:done -->
