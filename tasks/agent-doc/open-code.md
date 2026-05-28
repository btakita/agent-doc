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
### Re: #stqc #adcr-dep-audit results — opus-4-7

**Method:** outbound `crate::` references (top-level `use` + body refs) within each target module, plus inbound reference counts from the orchestration set (preflight, plan, cycle_state, queue_dispatch, route, ipc_socket, git, harness, session_check, write, capture). Excludes test/comment matches.

**Outbound (target → other crate modules):**

| Module | LOC | Internal deps | External deps | Class |
|---|---:|---|---|---|
| `crdt.rs` | 1,255 | — | `anyhow`, `yrs` | **leaf** |
| `model_tier.rs` | 1,088 | `component` | `anyhow`, `serde` | **leaf** |
| `pending.rs` | 3,565 | `component` | `anyhow` | **leaf** |
| `component.rs` | 1,706 | `new_boundary_id`, `format_boundary_marker` (free fns in `lib.rs`) | `anyhow`, `pulldown_cmark` | **near-leaf** |
| `frontmatter.rs` | 2,105 | `model_tier`, `project_config` | `anyhow`, `serde`, `uuid` | **near-leaf** |
| `template.rs` | 5,749 | `component`, `frontmatter`, `project_config`, lib boundary helpers | `anyhow`, `serde` | **near-leaf** |
| `diff.rs` | 4,456 | `component`, `pending`, `snapshot` | `anyhow`, `serde`, `similar` | **coupled** ⚠️ |

Helper: `project_config.rs` (474 LOC) — only depends on `frontmatter`. Forms a two-way ref pair with `frontmatter` (mutual `crate::` body references); both must move together.

**Inbound (orchestration → target, ref counts):**

```
              tmpl  diff  comp  fmtr  crdt  pend  mtier
preflight       3    25   50     4    —    39    —
session_check   1    27   14    24    —    15    —
write          66    45   22    10   20    31    —
git             4     9   13    11    5     3    —
route           3     4    4    —    —    —    —
cycle_state    —    —    —     1    —    —    —
queue_dispatch —    —    —     1    —    —    —
harness        —    —    —     1    —    —    —
capture        —    —    —     1    —    —    —
```

Heavy consumers — `write`, `preflight`, `session_check`, `git` — are exactly the orchestration modules. Target modules barely call back into orchestration. The seam is real.

**Classification:**

- **Pure / near-pure leaves** (safe to extract first, no seam decisions): `crdt`, `pending`, `model_tier`, `component`. Combined ~7.6k LOC. Only friction is the two free functions `new_boundary_id` and `format_boundary_marker` in `lib.rs` — relocate them into `agent-doc-core::id` or duplicate as a tiny inline.
- **Near-leaves with intra-target deps** (extract in same wave as their deps): `frontmatter` + `project_config` (must move together; mutual refs), `template` (needs `component`, `frontmatter`, `project_config`, lib boundary helpers). Combined ~8.3k LOC.
- **Coupled** ⚠️: `diff.rs`. Calls `snapshot::{load, save, path_for, resolve}` — these are file/git I/O, not pure data. `snapshot.rs` itself imports `git`, `claim`, `sessions`, `ops_log`, `secret_redact`, `fs_util`. Pulling `diff` into `agent-doc-core` as-is would drag the entire orchestration layer with it.

**diff.rs seam options** (see new backlog item `#adcr-snapshot-seam`):

1. Extract just the `Snapshot` data type (struct + serde) into `agent-doc-core`; keep `snapshot::{load, save, resolve, path_for}` in the orchestration crate; rewrite `diff.rs` to take a `&Snapshot` rather than calling loader functions. Lowest blast radius.
2. Define a `SnapshotPort` trait in `agent-doc-core`, implement it in orchestration. More flexible, more ceremony.
3. Split `diff.rs` into `diff_data.rs` (pure compare) + `diff_io.rs` (snapshot-backed) and only move the pure half. Probably cleanest given `diff.rs` is 4.5k LOC.

**Extraction order recommendation (informs #5x1r and #d97v):**

Wave 1 (no seam decisions): `crdt`, `model_tier`, `pending`, `component` + relocate boundary helpers.
Wave 2 (after Wave 1): `frontmatter` + `project_config` together.
Wave 3 (after Wave 2): `template`.
Wave 4 (after `#adcr-snapshot-seam` resolves): `diff` (or its pure half).

This makes Wave 1 the lowest-risk validation of the workspace structure proposed by `#d97v` before any seam work is required.

**Top of backlog after this cycle:** #adcr-snapshot-seam (new precondition for #adcr blocking diff extraction), then #5x1r.
### Re: #5x1r #adcr-api-surface — opus-4-7

Captured in `tasks/agent-doc/plan-agent-doc-core-extraction.md`. That file is now the umbrella plan for `#adcr` — it consolidates the `#stqc` audit, the API surface deliverable for this item, the workspace baseline contract that `#d97v` needs to satisfy, the `#rtx6` diff seam decision, and the wave order.

#### Surface (`agent-doc-core/src/lib.rs` draft)

Eight modules (`id`, `component`, `crdt`, `model_tier`, `pending`, `frontmatter`, `project_config`, `template`, `diff`), plus convenience re-exports of every public struct/enum from each module.

Re-export block (verbatim from plan):

```rust
pub use component::Component;
pub use crdt::CrdtDoc;
pub use diff::{
    DiffType, DiffClassification, PromptBearingChange, PromptBearingChangeKind,
    ParsedSlashCommands, OrchestrationRequest, OrchestrationRequestMode,
};
pub use frontmatter::{
    Frontmatter, AgentDocFormat, AgentDocWrite, CollaborationMode,
    CodexNetworkAccess, ResolvedMode, StreamConfig,
    PendingCaptureGuardMode, LintDialectMode,
};
pub use model_tier::{Tier, TierMap, ModelConfig, ModelSwitchScan};
pub use pending::{
    PendingItem, PendingState, PendingOp, PendingListMarker, TransitionResult,
    ShadowPendingItem, ShadowPendingReport,
    MalformedPendingItemLine, DroppedBacklogItem, DroppedBacklogReport,
};
pub use project_config::{
    ProjectConfig, GuardConfig, LintConfig,
    SshConfig, SshDocConfig, SshProfileConfig, ComponentConfig,
};
pub use template::{PatchBlock, TemplateInfo, ComponentInfo};
```

External deps the new crate needs: `anyhow`, `serde`, `uuid`, `pulldown_cmark`, `yrs`, `similar`. No `git2`, no `tokio`, no `nix`, no `tmux_interface` — those stay with orchestration.

#### Pure vs I/O split (the non-obvious calls)

The interesting design decisions are not the obvious data types — they're the functions that look pure but currently touch I/O. Plan §"Stays in main `agent-doc` crate" enumerates the wrappers:

- `frontmatter::ensure_session_for_file(&Path)`, `set_format_and_write(&Path, ...)`, `read_session_id(&Path)` — keep in main crate; the pure `parse` / `write` move into core.
- `template::template_info(&Path)` — main crate; pure inspector logic moves into core.
- `model_tier::detect_harness()` — env-coupled; recommend wrapping it as a thin orchestration helper that calls a pure `Tier::from_harness_str(&str)` in core. This keeps `agent-doc-core` env-agnostic.
- `project_config::load_project*` / `save_project` / `*_tmux_session` — main crate; pure parser/serializer (the `ProjectConfig` struct + serde derive) moves into core.

#### Boundary helpers relocation

`new_boundary_id`, `new_boundary_id_with_summary`, `format_boundary_marker`, and `BOUNDARY_ID_LEN` move from `lib.rs` into `agent-doc-core::id`. The main crate keeps `pub use agent_doc_core::id::*` so call sites in `component.rs`, `template.rs`, `boundary.rs`, etc. stay unchanged.

#### Diff carve-out (#rtx6 ranked options)

Documented at plan §"Diff seam decision". Recommendation: **option 1 — split `diff.rs` into `diff_data.rs` (pure, moves to core) + `diff_io.rs` (snapshot-backed, stays in main crate)**. Smallest blast radius; lets the four `snapshot::*` calls keep their current shape while still freeing the bulk of `diff.rs` (classification, slash-command parsing, prompt-bearing-change detection, annotation, comment stripping) for the editor-plugin/FFI target.

Decision deferred to after Wave 3 lands so we have real call-site data for `compute` / `compute_with_current` / `wait_for_stable_content`.

#### Acceptance (carried in plan)

1. `agent-doc-core` cold-builds in under 30% of the main-crate cold build time.
2. No `agent-doc-core` module imports any orchestration module (git, claim, sessions, ops_log, tmux, harness, route, ipc_socket, supervisor, cycle_state, preflight, plan, dispatch).
3. No `agent-doc-core` function takes `&Path` or runs `std::fs` / `std::process::Command`.
4. `make check` and `make tmux-ci` green at every wave boundary.

#### Wave ordering recap

Wave 1: `id` + `crdt` + `model_tier` + `pending` + `component` (~7.6k LOC, no seams).
Wave 2: `frontmatter` + `project_config` (must move together; mutual refs).
Wave 3: `template`.
Wave 4 (after #rtx6): `diff` (or just its pure half).

Plan-backed pending: I edited `#d97v` to cite the plan file path so the workspace scaffold work picks up the baseline contract.

**Top of backlog after this cycle:** #rtx6, then #d97v.
### Re: #d97v halt — opus-4-7

Stopping the auto-loop here. Two reasons:

1. **Session accretion escalated to `block`** — 161 exchange lines, 4 response sections, 11 cycles in 30 min (8 no-op closeouts). The binary's guidance is "Restart cleanly from the current committed boundary before continuing." That's a stronger signal than the `warn` level we've been working through.
2. **`#d97v` is not a low-tier preset-driven cycle.** The preset is `#spec-test-build-install-commit-push`. The scope from `plan-agent-doc-core-extraction.md` §workspace-scaffold is:
   - Convert `src/agent-doc/Cargo.toml` (which currently declares a `cdylib + lib + bin` package with a `[patch.crates-io]` for `tmux-router`) into a workspace root.
   - Create `src/agent-doc/agent-doc-core/` with its own `Cargo.toml` + stub `src/lib.rs`.
   - Capture `cargo build --release --timings` baselines for both crates — meaningful numbers require `cargo clean` first, which costs the next dev build several minutes of wall time.
   - `make check` (full suite, full clippy).
   - `cargo install --path .` (per release convention — both `.bin/agent-doc` symlink target and `~/.cargo/bin/agent-doc` need to refresh).
   - Manual testing gate in a live session per the release convention.
   - Commit + push.

   That's reasonable scoped work for an explicit operator-driven cycle, not for an auto-drained queue item where I can't pause for the manual testing gate.

**Recommendation:** compact this session first, then engage `#d97v` explicitly.

```
agent-doc compact /home/brian/work/btakita/agent-loop/tasks/agent-doc/open-code.md --commit
```

After that, either re-queue `[#d97v]` manually or invoke `/agent-doc tasks/agent-doc/open-code.md` with `do #d97v` to scope the next cycle deliberately. `#rtx6` (diff seam decision) is a pure design item and can drain through the queue at any tier; `#adcr` is the umbrella execute item and should not auto-loop until at least Wave 1 lands.

**Backlog left intact** — no `--done`, no `--pending-gate`. The queue head consumes `[#d97v]` as processed (you'll see queue advance to `[#adcr]` in preflight), but the pending item itself stays open for the explicit cycle.

**Not invoking `/loop`** so the queue does not auto-advance to `#adcr`.
<!-- agent:boundary:2fe88ef0 -->
<!-- /agent:exchange -->

## Queue

<!-- agent:queue auto -->
preset #spec-test-build-install-commit-push
- ~[#stqc]~
- ~[#5x1r]~
- ~[#d97v]~
- [#adcr]
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog -->
- [ ] [#rtx6] [recommended] #adcr-snapshot-seam Decide diff.rs ↔ snapshot.rs seam — either extract Snapshot data type into agent-doc-core and keep load/save/resolve/path_for in orchestration, or inject snapshot through a trait. Required before extracting diff.
- [ ] [#d97v] #adcr-workspace Convert src/agent-doc into Cargo workspace root; add agent-doc-core stub crate; record cargo build --timings baselines per tasks/agent-doc/plan-agent-doc-core-extraction.md
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
<!-- /agent:done -->
