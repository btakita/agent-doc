# Plan: agent-doc-core crate extraction

Umbrella plan for backlog item `#adcr`. Tracks the work to split `src/agent-doc/` into a Cargo workspace whose `agent-doc-core` member crate holds the pure document data layer, so editor plugins, FFI consumers, and isolated tests can compile and link without dragging in the full orchestration layer (route, git, tmux, IPC, harness probing).

## Status

| ID | Task | State |
|---|---|---|
| `#stqc` | Dep audit (this plan §Audit) | done 2026-05-28 |
| `#5x1r` | API surface (this plan §API surface) | done 2026-05-28 |
| `#d97v` | Workspace scaffold + compile-time baseline | open |
| `#rtx6` | `diff.rs` ↔ `snapshot.rs` seam decision | **decided 2026-05-28 — Option 1** (file split) |
| `#adcr` | Per-wave extraction | open (executes after the three above) |

## Audit summary (#stqc result)

Outbound `crate::` references from the seven target modules (top-level `use` + body refs):

| Module | LOC | Internal deps | Class |
|---|---:|---|---|
| `crdt.rs` | 1,255 | — | pure leaf |
| `model_tier.rs` | 1,088 | `component` | leaf |
| `pending.rs` | 3,565 | `component` | leaf |
| `component.rs` | 1,706 | `new_boundary_id` / `format_boundary_marker` (free fns in `lib.rs`) | near-leaf |
| `frontmatter.rs` | 2,105 | `model_tier`, `project_config` | near-leaf |
| `template.rs` | 5,749 | `component`, `frontmatter`, `project_config`, lib boundary helpers | near-leaf |
| `diff.rs` | 4,456 | `component`, `pending`, `snapshot` | **coupled** — `snapshot::{load, save, resolve, path_for}` pull in `git`, `claim`, `sessions`, `ops_log`, `secret_redact`, `fs_util` |

Helper module `project_config.rs` (474 LOC) only depends on `frontmatter`. It forms a two-way reference pair with `frontmatter` and must move in the same wave.

Inbound from orchestration (preflight, plan, cycle_state, queue_dispatch, route, ipc_socket, git, harness, session_check, write, capture) — heavy consumers `write`, `preflight`, `session_check`, `git` together account for the majority of refs. Target modules barely call back into orchestration: the only outbound coupling is the `snapshot` calls in `diff.rs` and the boundary helpers used by `component`/`template`.

## API surface (#5x1r)

Proposed `agent-doc-core/src/lib.rs`:

```rust
//! agent-doc-core — pure document data layer for agent-doc.
//!
//! No filesystem, git, tmux, IPC, or harness probing. Consumers pass
//! parsed content in and receive parsed/mutated content back.

pub mod id;            // boundary id helpers (moved from lib.rs)
pub mod component;
pub mod crdt;
pub mod model_tier;
pub mod pending;
pub mod frontmatter;
pub mod project_config;
pub mod template;
pub mod diff;          // pure half only — see #rtx6

// Convenience re-exports for common types.
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

### Pure (stays in `agent-doc-core`)

- All structs and enums listed above.
- All free functions whose signature takes `&str` / `&[u8]` / parsed data and returns the same. Includes: `component::parse`, `component::strip_comments`, `crdt::merge`/`compact`/`dedup_adjacent_blocks`, `pending::*` ops, `template::parse_patches`/`apply_patches`/`apply_patches_with_overrides`/all `repair_*`/`normalize_editor_visible_template_structure`/all `reposition_boundary_*`, `frontmatter::parse`/`write`/`merge_fields`/`set_*`, `model_tier::resolve_tier_to_model`/`canonical_model_name`/`parse_model_arg`/`is_bare_model_name`/`extract_model_component`/`component_value_to_tier`/`suggested_tier`/`scan_model_switch`/`compose_effective_tier`/`tier_from_model_name`, `diff::strip_comments`/`annotate_diff`/`extract_inline_annotations`/`classify_diff`/`unified_diff_from_contents`/`prompt_prefix_normalization_targets`/`classify_prompt_bearing_changes`/`change_is_managed_state_only`/`is_builtin_command`/`parse_slash_commands*`/`detect_orchestration_request`/`detect_queue_trigger`/`detect_prompt_preset_requests`/`extract_prompt_preset_requests_from_text`/`extract_imperative_directives`/`diff_contains_imperative_directive`/`detect_exchange_compaction_request`/`extract_required_response_blocks`/`format_*`.

### Stays in main `agent-doc` crate (I/O wrappers)

Pure parsers live in `agent-doc-core`; these are thin adapters that read/write disk, git, or env:

- `frontmatter::ensure_session_for_file(&Path)`, `frontmatter::set_format_and_write(&Path, ...)`, `frontmatter::read_session_id(&Path)` — wrap the pure `parse` / `write` with file I/O.
- `template::template_info(&Path)` — reads file then delegates to pure inspector.
- `diff::compute(&Path)`, `diff::compute_with_current(&Path)`, `diff::wait_for_stable_content(&Path, &str)`, `diff::is_stale_snapshot(...)`, `diff::run(&Path, bool)` — depend on snapshot I/O (see #rtx6).
- `model_tier::detect_harness()` — reads env (`AGENT_DOC_HARNESS` etc.). Could stay in `agent-doc-core` if the env coupling is acceptable, but cleaner as an orchestration wrapper that calls a `Tier::from_harness_str(&str)` pure function in core.
- `project_config::load_project*`, `save_project`, `*_tmux_session` — filesystem I/O; the pure parser/serializer lives in core.

### Boundary helpers

Move `new_boundary_id`, `new_boundary_id_with_summary`, `format_boundary_marker` (and the `BOUNDARY_ID_LEN` const) from `lib.rs` into a new `agent-doc-core::id` module. The functions only use `uuid::Uuid` — no orchestration dependency. The main crate can `pub use agent_doc_core::id::*` to preserve current call sites.

## Workspace scaffold (#d97v)

Convert `src/agent-doc/` into a Cargo workspace root:

```text
src/agent-doc/
  Cargo.toml          # workspace = { members = ["agent-doc-core", "."] }
  src/                # main crate stays here for now (executable + orchestration)
  agent-doc-core/
    Cargo.toml        # lib, deps: anyhow, serde, uuid, pulldown_cmark, yrs, similar
    src/lib.rs        # surface from #5x1r
    src/...           # extracted modules (initially empty stubs)
```

Capture two baselines before extraction begins:

1. `cargo build -p agent-doc --release --timings` — current cold-build time.
2. `cargo build -p agent-doc-core --release --timings` (against the empty stub) — establishes the floor for "extracted-only" compile time.

These two numbers, recorded in `agent-doc-core/COMPILE_BASELINE.md`, gate the work: if a wave makes either build *slower* without offsetting LOC consolidation, that wave is rejected before merge.

## Diff seam decision (#rtx6 — precondition for diff extraction)

`diff.rs` calls four `snapshot::*` functions (`load`, `save`, `path_for`, `resolve`). These are file/git I/O and cannot move into `agent-doc-core` as-is. Three options ranked by blast radius (smallest first):

1. **Split** — partition `diff.rs` into `diff_data.rs` (pure compare, classification, slash-command parsing, prompt-bearing-change detection) and `diff_io.rs` (snapshot-backed `compute*`, `run`, `wait_for_stable_content`). Only `diff_data.rs` moves into core. The I/O half stays in the main crate and calls into both `agent_doc_core::diff` and `snapshot`. **Recommended.**
2. **Snapshot data type extraction** — move only the `Snapshot` struct (and pure serde paths) into core; keep `load`/`save`/`resolve`/`path_for` in orchestration. Rewrite `diff.rs` to accept `&Snapshot` rather than calling loader functions. Requires touching every call site of `diff::compute*` to thread the snapshot through.
3. **Trait port** — define `trait SnapshotPort` in core, implement in orchestration. Maximum flexibility, but adds dyn-dispatch ceremony for one consumer; over-engineered for a single-implementer port.

**Decision (2026-05-28, `#rtx6`):** Option 1. Call-site survey from `src/agent-doc/src/` showed 6 production callers of `diff::compute*` / `diff::is_stale_snapshot` / `diff::run` (stream.rs:134, run.rs:274+374, preflight.rs:1721+1850, main.rs:1657, plan.rs:154). Option 1 preserves every one of those call sites unchanged; Options 2 and 3 require updates to all 6. Wave 4 implementation: keep the pure half in `agent_doc_core::diff`; rename current `diff.rs` to `diff_io.rs` and strip it to the snapshot-backed functions; re-export from `agent_doc::diff` so callers compile unchanged.

## Wave plan (#adcr execution)

Pre-req: `#d97v` workspace scaffold + baselines.

- **Wave 1** — pure leaves, no seam decisions:
  - `crate::id` (boundary helpers from `lib.rs`)
  - `component`
  - `crdt`
  - `model_tier` (with `detect_harness` left as a wrapper in main crate)
  - `pending`
  - ~7.6k LOC.
- **Wave 2** — `frontmatter` + `project_config` together (mutual refs). ~2.6k LOC. I/O wrappers stay in main crate.
- **Wave 3** — `template`. ~5.7k LOC. `template_info(&Path)` wrapper stays in main crate.
- **Wave 4** — `diff` per `#rtx6` decision. ~4.5k LOC (or its pure half).

Each wave: extract → update inbound call sites → `cargo build -p agent-doc-core --timings` (record) → full `make check` → commit.

## Acceptance

- `agent-doc-core` compiles standalone in under 30% of the main-crate cold build (measured against the `#d97v` baseline).
- No `agent-doc-core` module imports `git`, `claim`, `sessions`, `ops_log`, `tmux`, `harness`, `route`, `ipc_socket`, `supervisor`, `cycle_state`, `preflight`, `plan`, `dispatch`, or any other orchestration module.
- `agent-doc-core` exposes no functions that take `&Path` or perform `std::fs` / `std::process::Command` calls.
- All existing `agent-doc` CLI behavior unchanged — `make check` and `make tmux-ci` green at every wave boundary.
