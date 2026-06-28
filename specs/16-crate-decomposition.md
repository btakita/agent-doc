# Crate Decomposition Authority

This spec owns the split of former God-crate responsibilities. It cross-links
the realtime document state machine in
[Real-Time Workflow Authority](14-realtime-workflow.md) and the turn lifecycle
state machine in [Turn Lifecycle Authority](15-turn-lifecycle.md).

`agent-doc-core` and `agent-doc-orchestration` are transitional aggregation
crates. They may keep compatibility re-exports and adapter code while logic is
being extracted, but new domain policy must land in a focused crate unless the
change explicitly documents why extraction is unsafe for that patch.

There is no `agent-doc-orchestrator` crate today; discussion of the
"orchestrator" God crate refers to `agent-doc-orchestration`.

## Hard Rules

- Operator-visible document authority stays out of turn and harness crates.
- Merge logic stays pure and must not commit, spawn processes, open IPC, or read
  clocks.
- Turn lifecycle owns commit/no-commit decisions, but not realtime source
  authority.
- Element semantics belong to `agent-doc-element-*` crates or plugin element
  descriptors.
- `agent-doc-core` must trend toward a compatibility facade, not collect new
  document behavior.
- `agent-doc-orchestration` must trend toward wiring/adapters, not own policy
  for document merge, element semantics, turn lifecycle, tmux model, supervisor
  model, or controller CAS semantics.

## Current Extraction Map

| Responsibility | Destination | Current status |
|---|---|---|
| Element descriptors and local element models | `agent-doc-element-*` plus `agent-doc-element-registry` | Active. Queue item lifecycle now lives in `agent-doc-element-queue`; `agent-doc-core` re-exports it temporarily. |
| Cross-element document projection | `agent-doc-document` | Active for queue projection and element-model composition. |
| Pure merge / conflict policy | `agent-doc-merge` | Active facade over semantic and cell merge; remaining CRDT helpers should move here or into a CRDT-specific pure crate. |
| Turn lifecycle state machine | `agent-doc-turn` | Active. Lifecycle consumes realtime handoff proof and owns commit policy. |
| Executor vocabulary | `agent-doc-turn-executor` | Active. |
| Tmux observations and command effects | `agent-doc-tmux`, `agent-doc-tmux-commands`, `agent-doc-tmux-io`, `agent-doc-turn-executor-tmux` | Active starter crates; orchestration still has transitional tmux code. |
| SQLite-backed persistence | `agent-doc-sqlite` | Active. |
| Supervisor model and process effects | `agent-doc-supervisor`, `agent-doc-supervisor-process` | Planned; do not add new supervisor policy to orchestration except as a bridge. |
| Controller RPC/CAS | `agent-doc-controller` | Planned; durable controller state should not be called PCP. |
| Document realtime scheduler | `agent-doc-document-realtime` | Planned; owns editor/disk epochs, apply scheduling, parse projections, and verification, using lazily-rs. |

## Core Split Targets

The following `agent-doc-core` modules are explicitly scheduled for extraction:

| Current module | Target crate | Notes |
|---|---|---|
| `component.rs` | `agent-doc-component` or `agent-doc-element` submodule | Component parsing is element/document syntax. Keep parser pure; no file IO. |
| `pending.rs` | `agent-doc-element-backlog` / `agent-doc-tracked-work` | Tracked work parsing and lifecycle should be shared by backlog, review, icebox, and done. |
| `queue_item_lifecycle.rs` | `agent-doc-element-queue` | Extracted. Core path is compatibility only. |
| `template.rs` | `agent-doc-template` | Split pure patch parsing from any path/file helpers. |
| `crdt.rs`, `crdt_sync.rs`, `cell_doc.rs` | `agent-doc-merge` or `agent-doc-document-crdt` | Pure CRDT/merge math may move to merge; sync/transport belongs to document realtime. |
| `diff.rs` | `agent-doc-diff` or `agent-doc-document` | Keep prompt-bearing classification pure; IO remains outside. |
| `frontmatter.rs` | `agent-doc-document` or `agent-doc-frontmatter` | Pure parsing may move; file-backed wrappers stay effectful. |
| `ffi.rs` | `agent-doc-ffi` | C/JNA ABI surface should not force all core users to depend on FFI helpers. |

Until a target crate owns a module, `agent-doc-core` may expose the old module
path for compatibility. Compatibility modules should say where the authority now
lives and should avoid new behavior.

## Orchestration Split Targets

The following `agent-doc-orchestration` areas are explicitly scheduled for
extraction:

| Current area | Target crate | Notes |
|---|---|---|
| `write`, `compact`, editor convergence | `agent-doc-document-realtime` plus `agent-doc-merge` | Realtime scheduling, owner selection, delivery, and verification should leave orchestration. |
| `preflight`, queue maintenance | `agent-doc-document`, `agent-doc-element-queue`, `agent-doc-turn` | Preflight consumes parse/projection facts; it should not own queue semantics. |
| `start`, `supervisor`, stale actor reaping | `agent-doc-supervisor`, `agent-doc-supervisor-process`, `agent-doc-controller` | Distinguish pure supervisor state from process effects and controller storage. |
| `route`, `project_controller`, `state_store` | `agent-doc-controller` | Route should become an adapter to controller admission and actor bindings. |
| `tmux`, pane probing, command execution | `agent-doc-tmux-*`, `agent-doc-turn-executor-tmux` | Use pure command builders and typed observations. |
| `session_check`, closeout recovery classification | `agent-doc-turn` plus document realtime projections | Session check consumes lifecycle/realtime facts; it should not re-own source authority. |

## Extraction Policy

When adding a new feature, choose the narrowest crate that owns the policy:

1. Pure element rule: add to the relevant `agent-doc-element-*` crate.
2. Cross-element document projection: add to `agent-doc-document`.
3. Pure merge behavior: add to `agent-doc-merge`.
4. Editor/disk realtime scheduling or parse projection: add to
   `agent-doc-document-realtime` once the crate exists; until then isolate code
   behind a module that can be moved without changing semantics.
5. Turn admission, closeout state, and commit policy: add to `agent-doc-turn`.
6. Tmux state/commands/effects: add to the `agent-doc-tmux*` family.
7. Supervisor/controller state: add to the planned supervisor/controller crates.

Adding new policy directly to `agent-doc-core` or `agent-doc-orchestration`
requires an explicit comment or spec note naming the destination crate and why
the policy cannot move in the same patch.
