# PRD: Crate Decomposition

## Problem

`agent-doc-orchestration` is still an aggregation crate. `agent-doc-core` was
deleted after its policy moved into focused crates; it must not come back as an
empty facade. Remaining orchestration policy should live in focused domain
crates, because mixed effect/policy modules make realtime and turn invariants
easier to regress accidentally.

This PRD tracks the extraction work needed to keep deleted core APIs gone and
make orchestration small enough that it is an adapter rather than a God crate.

## Goals

- Keep `agent-doc-core` deleted; move new domain policy out of
  `agent-doc-orchestration`.
- Keep durable system rules in the canonical specs:
  [Real-Time Workflow Authority](../../specs/14-realtime-workflow.md) and
  [Turn Lifecycle Authority](../../specs/15-turn-lifecycle.md).
- Keep extraction work visible, testable, and incrementally shippable.
- Treat the focused crates as the greenfield Rust API; internal callers import
  those crates directly instead of preserving old facade paths.

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
| Element descriptors and local element models | `agent-doc-element-*` plus `agent-doc-element-registry` | Active. Queue item lifecycle now lives in `agent-doc-element-queue`; orchestration no longer owns the queue item state machine. |
| Queue syntax, scheduling, and drainability | `agent-doc-queue` | Active. Pure queue parsing, preemption/edit-owner rules, and queue-continuation drainability/noise/context-reset policy live here; orchestration keeps snapshot, marker, controller-state, and file-IO adapters. |
| Cross-element document projection | `agent-doc-document` | Active for queue projection and element-model composition. |
| Work graph / Auto-DAG scheduling | `agent-doc-work-graph` | Active. Source-agnostic graph classification and rendering live here so markdown documents, cross-document sources, and external PM adapters can feed the same dependency model. |
| Template-mode patch parsing, repair, and replay validation | `agent-doc-template` | Active. Pure patch parsing, replay-payload validation, component mutation, boundary repositioning, and duplicate prompt/tail repair live here; orchestration keeps file-backed config/document IO. |
| Diff annotation and prompt-bearing classification | `agent-doc-diff` | Active. Pure comment stripping, unified-diff helpers, slash/preset/directive extraction, and prompt-bearing change classification live here. |
| Frontmatter and project config parsing | `agent-doc-frontmatter` | Active. Document frontmatter schema, project config types, and pure parsing/serialization helpers live here; orchestration keeps file-backed IO. |
| Editor visual syntax tokenization | `agent-doc-syntax` | Active. Pure visual token spans for editor integrations live here; FFI remains the ABI adapter. |
| Log timestamp formatting/parsing | `agent-doc-log-time` | Active. Pure log timestamp helpers live here; orchestration log writers/readers call it directly and must not re-export it through `ops_log`. |
| Exchange topic-section parsing | `agent-doc-topic` | Active. Pure `### Re:` exchange section splitting and boundary-tail stripping live here; compaction/archive code consumes the parser directly. |
| C/JNA editor ABI | `agent-doc-ffi` | Active. Pure C-ABI exports for editor integrations live here and depend on focused crates directly; the main crate remains the cdylib adapter. |
| Pure merge / conflict policy | `agent-doc-merge` | Active. Semantic merge, frontmatter-aware CRDT merge, CRDT state-vector sync, per-cell merge projection, merge write-ownership phases, events, transition table, liveness facts, and disk-write predicate live here; orchestration imports `agent_doc_merge::ownership` directly and only adapts plugin-owner/op-capture sidecar IO into that pure model. The deleted `merge_control_state_machine` facade must not return. |
| Turn lifecycle, operation log, and turn-scope affectedness | `agent-doc-turn` | Active. Lifecycle consumes realtime handoff proof and owns commit policy. Cycle phase transitions, closeout response heuristics, operation-log data types, turn-scope manifests, affectedness classification, and queue-continuation stall classification live here; orchestration imports drain-stall policy directly from `agent_doc_turn::drain_stall` and owns only sidecar persistence and command adapters. |
| Executor vocabulary | `agent-doc-turn-executor` | Active. Turn executor model vocabulary lives here, and idle-queue drain/context-clear readiness policy now lives in `agent-doc-turn-executor::idle_queue`; orchestration should call the focused API directly. |
| Tmux observations and command effects | `agent-doc-tmux`, `agent-doc-tmux-commands`, `agent-doc-tmux-io`, `agent-doc-turn-executor-tmux` | Active starter crates; orchestration still has transitional tmux code. |
| SQLite-backed persistence | `agent-doc-sqlite` | Active. |
| Document realtime scheduler | `agent-doc-document-realtime` | Active. Owns editor/disk read-authority policy, editor/disk authority states, apply scheduling states, verification handoff vocabulary, pure live-editor delivery target selection, write/watch authority predicates, and the inter-turn document convergence gate. |
| Editor debounce and sidecars | `agent-doc-debounce` | Active. Typing debounce state, live-buffer sidecars, editor sync barriers, write-provenance records, and live-buffer classification diagnostics live here; orchestration consumes the sidecar API directly. |
| Supervisor model and process effects | `agent-doc-supervisor`, `agent-doc-supervisor-process` | Active. Pure state/recovery decisions, supervisor config precedence, process lifecycle decisions, harness-change restart policy, post-child-exit run-loop dispatch, and controller handoff state now live in `agent-doc-supervisor`; process handoff state for `execve` re-entry now lives in `agent-doc-supervisor-process`. The old `start::decisions` facade is deleted; orchestration imports the focused crates directly. |
| Controller RPC/CAS | `agent-doc-controller` | Active. Controller stores/apply decisions; it does not own supervisor policy. Pure dispatch admission helpers for in-flight coalescing, operator reopen classification, and stale-generation redirect parsing now live in `agent-doc-controller::dispatch`; orchestration imports them directly and the RPC layer remains only the socket/process adapter. |

## Core Split Targets

| Current module | Target crate | Notes |
|---|---|---|
| `component.rs` | `agent-doc-element::element` | Extracted. Element parsing is element/document syntax; keep parser pure and file-IO free. |
| `pending.rs` | `agent-doc-element-backlog` / `agent-doc-tracked-work` | Tracked work parsing and lifecycle should be shared by backlog, review, icebox, and done. |
| `queue_item_lifecycle.rs` | `agent-doc-element-queue` | Extracted. |
| `template.rs` | `agent-doc-template` | Extracted. Pure patch parsing, component mutation, boundary repositioning, and repair helpers live in `agent-doc-template`; file-backed config/document IO stays in orchestration adapters. |
| `replay_guard.rs` | `agent-doc-template` | Extracted. Replay payload shape validation lives beside template patch parsing because patch-bearing replay depends on `parse_patches`. |
| `crdt.rs`, `crdt_sync.rs`, `cell_doc.rs` | `agent-doc-merge` | Extracted. Pure CRDT/merge math, per-cell projection, and state-vector sync primitives live in `agent-doc-merge`; transport/authority adapters stay in orchestration/realtime layers. |
| `diff.rs` | `agent-doc-diff` | Extracted. Pure prompt-bearing classification, slash/preset/directive extraction, and diff annotation live in `agent-doc-diff`; IO remains in orchestration adapters. |
| `frontmatter.rs` | `agent-doc-frontmatter` | Extracted together with pure project config parsing. File-backed wrappers stay effectful in orchestration adapters. |
| `op_log.rs`, `turn_scope.rs` | `agent-doc-turn` | Extracted. Operation-log data types, turn-scope manifests, and affectedness classification live with the turn lifecycle model; durable sidecar/sqlite IO stays in orchestration/sqlite adapters. |
| `heuristics.rs` | `agent-doc-turn` | Extracted. Pending-capture recommendation detection is turn closeout policy; orchestration only applies the resulting signal to guards. |
| `syntax.rs` | `agent-doc-syntax` | Extracted. Editor-facing visual tokenization is pure document syntax; FFI/editor integrations stay as adapters. |
| `topic.rs` | `agent-doc-topic` | Extracted. Exchange topic section parsing is pure text segmentation shared by compaction/archive flows. |
| `ffi.rs` | `agent-doc-ffi` | Extracted. C/JNA ABI exports depend on focused crates directly. |

`agent-doc-core` itself is deleted. Focused crates are the only Rust API
surface for extracted policy.

## Orchestration Split Targets

| Current area | Target crate | Notes |
|---|---|---|
| `write`, `compact`, editor convergence | `agent-doc-document-realtime` plus `agent-doc-merge` | Realtime scheduling, owner selection, delivery, and verification should leave orchestration. Live editor delivery target selection, write/watch authority predicates, and the inter-turn convergence gate are already pure in `agent-doc-document-realtime`; merge write-ownership is now pure in `agent-doc-merge`; orchestration is only adapting current sidecar IO into those decisions. |
| Editor debounce, live-buffer sidecars, write provenance | `agent-doc-debounce` | Extracted. Cross-process typing markers, durable live-buffer snapshots, editor sync barrier polling, write-provenance sidecars, and live-buffer classification diagnostics live in the focused sidecar crate. |
| `preflight`, queue maintenance | `agent-doc-document`, `agent-doc-element-queue`, `agent-doc-queue`, `agent-doc-turn`, `agent-doc-work-graph` | Preflight consumes parse/projection facts; it should not own queue semantics. Completed: queue item lifecycle moved to `agent-doc-element-queue`; queue continuation drainability/noise/context-reset policy moved to `agent-doc-queue`; pure Auto-DAG classification/rendering moved to `agent-doc-work-graph`; cycle phase transitions moved to `agent-doc-turn`; orchestration keeps command/preflight/sidecar adapters. |
| `start`, `supervisor`, stale actor reaping | `agent-doc-supervisor`, `agent-doc-supervisor-process`, `agent-doc-controller` | Distinguish pure supervisor state from process effects and controller storage. Completed: recycle/install/restart lifecycle policy, retry bounds, boot-resume policy, stale-drain yield policy, supervisor config precedence, agent-change restart policy, post-child-exit dispatch, and controller handoff policy moved to `agent-doc-supervisor`; `execve` re-entry handoff state moved to `agent-doc-supervisor-process`; the transitional `start::decisions` facade is gone, and orchestration keeps only idle-watch/process/tmux adapters. |
| Idle queue / executor readiness | `agent-doc-turn-executor` | Completed for pure idle queue drain, context-reset, clear-settle, dedup, and between-turn enqueue planning. Orchestration should only observe pane/editor facts and call the executor policy. |
| `route`, `project_controller`, `state_store` | `agent-doc-controller` | Route should become an adapter to controller admission and actor bindings. Started: dispatch coalescing and stale-generation retry parsing moved to `agent-doc-controller::dispatch`; stale-queue recovery still depends on orchestration flow outcome and should be split next. |
| `tmux`, pane probing, command execution | `agent-doc-tmux-*`, `agent-doc-turn-executor-tmux` | Use pure command builders and typed observations. |
| `session_check`, closeout recovery classification | `agent-doc-turn` plus document realtime projections | Session check consumes lifecycle/realtime facts; it should not re-own source authority. Queue-continuation stall classification is now in `agent-doc-turn`; orchestration persists the one-shot marker. |

## Acceptance Criteria

- `agent-doc-core` is absent from the workspace and from crate dependencies.
  New policy lands in a focused crate or has an explicit temporary bridge
  comment naming the target.
- `agent-doc-orchestration` no longer owns new domain policy for merge,
  realtime document authority, turn lifecycle, tmux state, supervisor state, or
  controller CAS.
- Internal and in-repo public Rust paths use focused crates directly; extracted
  APIs are not kept alive through `agent-doc-core` or root-crate facades.
- Each extracted crate has at least one meaningful unit test or compile-time
  dependency-boundary test.
- The workspace builds and the focused extraction tests pass.

## Deprecating `agent-doc-orchestration`

`agent-doc-orchestration` should be treated as transitional. The honest target is
not to give it a better broad name, but to make it small enough that it can be
deprecated without losing a domain boundary. During the transition it may:

- wire CLI/start/run-loop effects together;
- adapt existing sidecar, tmux, process, and file IO into focused crates;
- adapt existing sidecar, tmux, process, and file IO into focused crates.

It should not own durable policy for document realtime, merge, turn lifecycle,
element semantics, executor readiness, supervisor lifecycle, process handoff, or
controller state. Once those responsibilities are absent, replace remaining
callers with focused crate APIs and mark the crate deprecated or remove it.

## Completion Signal

This PRD is complete when `agent-doc-core` remains deleted and
`agent-doc-orchestration` is no longer a God crate by behavior: orchestration may
still adapt effectful boundaries, but its remaining modules do not define durable
domain policy that belongs to the focused crates above.
