# PRD: Crate Decomposition

## Problem

`agent-doc-core` and `agent-doc-orchestration` are still aggregation crates.
They contain policy that should live in focused domain crates, which makes the
system harder to reason about and makes realtime/turn invariants easier to
regress accidentally.

This PRD tracks the extraction work needed to make those transitional crates
small enough that they are adapters/facades rather than God crates.

## Goals

- Move new domain policy out of `agent-doc-core` and `agent-doc-orchestration`.
- Keep durable system rules in the canonical specs:
  [Real-Time Workflow Authority](../../specs/14-realtime-workflow.md) and
  [Turn Lifecycle Authority](../../specs/15-turn-lifecycle.md).
- Keep extraction work visible, testable, and incrementally shippable.
- Preserve compatibility re-exports while callers migrate.

## Non-Goals

- Do not complete every extraction in one patch.
- Do not rename `agent-doc-orchestration` in the same patch as behavior
  extraction. Deprecation should be an explicit final step after callers are
  migrated to focused crates.
- Do not move effectful code into pure crates.
- Do not weaken the operator-visible document authority invariant to make
  extraction easier.

## Current Extraction Map

| Responsibility | Destination | Current status |
|---|---|---|
| Element descriptors and local element models | `agent-doc-element-*` plus `agent-doc-element-registry` | Active. Queue item lifecycle now lives in `agent-doc-element-queue`; orchestration no longer owns the queue item state machine. `agent-doc-core` re-exports its older lifecycle facade temporarily. |
| Cross-element document projection | `agent-doc-document` | Active for queue projection, element-model composition, and pure Auto-DAG analysis/rendering. |
| Pure merge / conflict policy | `agent-doc-merge` | Active facade over semantic and cell merge. Merge write-ownership phases, events, transition table, liveness facts, and disk-write predicate now live here; orchestration only adapts plugin-owner sidecar IO into that pure model. Remaining CRDT helpers should move here or into a CRDT-specific pure crate. |
| Turn lifecycle state machine | `agent-doc-turn` | Active. Lifecycle consumes realtime handoff proof and owns commit policy. Cycle phase transitions and queue-continuation stall classification also live here; orchestration owns only sidecar persistence and command adapters. |
| Executor vocabulary | `agent-doc-turn-executor` | Active. Turn executor model vocabulary lives here, and idle-queue drain/context-clear readiness policy now lives in `agent-doc-turn-executor::idle_queue`; orchestration keeps compatibility re-exports for existing start/idle-watch callers. |
| Tmux observations and command effects | `agent-doc-tmux`, `agent-doc-tmux-commands`, `agent-doc-tmux-io`, `agent-doc-turn-executor-tmux` | Active starter crates; orchestration still has transitional tmux code. |
| SQLite-backed persistence | `agent-doc-sqlite` | Active. |
| Document realtime scheduler | `agent-doc-document-realtime` | Active. Owns editor/disk authority states, apply scheduling states, verification handoff vocabulary, pure live-editor delivery target selection, and the inter-turn document convergence gate. |
| Supervisor model and process effects | `agent-doc-supervisor`, `agent-doc-supervisor-process` | Active. Pure state/recovery decisions, supervisor config precedence, process lifecycle decisions, harness-change restart policy, post-child-exit run-loop dispatch, and controller handoff state now live in `agent-doc-supervisor`; process handoff state for `execve` re-entry now lives in `agent-doc-supervisor-process`. |
| Controller RPC/CAS | `agent-doc-controller` | Active. Controller stores/apply decisions; it does not own supervisor policy. Pure dispatch admission helpers for in-flight coalescing, operator reopen classification, and stale-generation redirect parsing now live in `agent-doc-controller::dispatch`; orchestration's RPC layer remains the socket/process adapter. |

## Core Split Targets

| Current module | Target crate | Notes |
|---|---|---|
| `component.rs` | `agent-doc-element::element` | Extracted. Element parsing is element/document syntax; keep parser pure and file-IO free. |
| `pending.rs` | `agent-doc-element-backlog` / `agent-doc-tracked-work` | Tracked work parsing and lifecycle should be shared by backlog, review, icebox, and done. |
| `queue_item_lifecycle.rs` | `agent-doc-element-queue` | Extracted. Core path is compatibility only. |
| `template.rs` | `agent-doc-template` | Split pure patch parsing from any path/file helpers. |
| `crdt.rs`, `crdt_sync.rs`, `cell_doc.rs` | `agent-doc-merge` or `agent-doc-document-crdt` | Pure CRDT/merge math may move to merge; sync/transport belongs to document realtime. |
| `diff.rs` | `agent-doc-diff` or `agent-doc-document` | Keep prompt-bearing classification pure; IO remains outside. |
| `frontmatter.rs` | `agent-doc-document` or `agent-doc-frontmatter` | Pure parsing may move; file-backed wrappers stay effectful. |
| `ffi.rs` | `agent-doc-ffi` | C/JNA ABI surface should not force all core users to depend on FFI helpers. |

## Orchestration Split Targets

| Current area | Target crate | Notes |
|---|---|---|
| `write`, `compact`, editor convergence | `agent-doc-document-realtime` plus `agent-doc-merge` | Realtime scheduling, owner selection, delivery, and verification should leave orchestration. Live editor delivery target selection and the inter-turn convergence gate are already pure in `agent-doc-document-realtime`; merge write-ownership is now pure in `agent-doc-merge`; orchestration is only adapting current sidecar IO into those decisions. |
| `preflight`, queue maintenance | `agent-doc-document`, `agent-doc-element-queue`, `agent-doc-turn` | Preflight consumes parse/projection facts; it should not own queue semantics. Completed: queue item lifecycle moved to `agent-doc-element-queue`; pure Auto-DAG analysis/rendering moved to `agent-doc-document`; cycle phase transitions moved to `agent-doc-turn`; orchestration keeps command/preflight/sidecar adapters. |
| `start`, `supervisor`, stale actor reaping | `agent-doc-supervisor`, `agent-doc-supervisor-process`, `agent-doc-controller` | Distinguish pure supervisor state from process effects and controller storage. Completed: recycle/install/restart lifecycle policy, retry bounds, boot-resume policy, stale-drain yield policy, supervisor config precedence, agent-change restart policy, post-child-exit dispatch, and controller handoff policy moved to `agent-doc-supervisor`; `execve` re-entry handoff state moved to `agent-doc-supervisor-process`; orchestration keeps idle-watch/process/tmux adapters. |
| Idle queue / executor readiness | `agent-doc-turn-executor` | Completed for pure idle queue drain, context-reset, clear-settle, dedup, and between-turn enqueue planning. Orchestration should only observe pane/editor facts and call the executor policy. |
| `route`, `project_controller`, `state_store` | `agent-doc-controller` | Route should become an adapter to controller admission and actor bindings. Started: dispatch coalescing and stale-generation retry parsing moved to `agent-doc-controller::dispatch`; stale-queue recovery still depends on orchestration flow outcome and should be split next. |
| `tmux`, pane probing, command execution | `agent-doc-tmux-*`, `agent-doc-turn-executor-tmux` | Use pure command builders and typed observations. |
| `session_check`, closeout recovery classification | `agent-doc-turn` plus document realtime projections | Session check consumes lifecycle/realtime facts; it should not re-own source authority. Queue-continuation stall classification is now in `agent-doc-turn`; orchestration persists the one-shot marker. |

## Acceptance Criteria

- `agent-doc-core` no longer owns new document policy; new policy lands in a
  focused crate or has an explicit temporary bridge comment naming the target.
- `agent-doc-orchestration` no longer owns new domain policy for merge,
  realtime document authority, turn lifecycle, tmux state, supervisor state, or
  controller CAS.
- Existing public paths that are not yet migrated continue to work through
  compatibility re-exports.
- Each extracted crate has at least one meaningful unit test or compile-time
  dependency-boundary test.
- The workspace builds and the focused extraction tests pass.

## Deprecating `agent-doc-orchestration`

`agent-doc-orchestration` should be treated as transitional. The honest target is
not to give it a better broad name, but to make it small enough that it can be
deprecated without losing a domain boundary. During the transition it may:

- wire CLI/start/run-loop effects together;
- adapt existing sidecar, tmux, process, and file IO into focused crates;
- re-export compatibility paths while callers migrate.

It should not own durable policy for document realtime, merge, turn lifecycle,
element semantics, executor readiness, supervisor lifecycle, process handoff, or
controller state. Once those responsibilities are absent, replace remaining
callers with focused crate APIs and mark the crate deprecated or remove it.

## Completion Signal

This PRD is complete when `agent-doc-core` and `agent-doc-orchestration` are no
longer God crates by behavior: they may still re-export or adapt, but their
remaining modules do not define durable domain policy that belongs to the
focused crates above.
