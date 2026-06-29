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
| Element descriptors and local element models | `agent-doc-element-*` plus `agent-doc-element-registry` | Active. Queue item lifecycle now lives in `agent-doc-element-queue`; review-list projection/filtering and ungate-task planning now live in `agent-doc-element-review`; orchestration no longer owns those element state/projection models. |
| Queue syntax, scheduling, and drainability | `agent-doc-queue` | Active. Pure queue parsing, id-backed `do #id` directive target extraction, queue command/prompt classification, queue response heading/head-id matching, queue prompt preservation identity matching, queue deletion identity/count comparison, preemption/edit-owner rules, queue-continuation drainability/noise/context-reset policy, operator-authored prompt identity detection, and continuation guidance wording live here; orchestration keeps snapshot, marker, controller-state, lifecycle, mutation, and file-IO adapters. The old manual-addition compatibility shim is deleted. |
| Cross-element document projection | `agent-doc-document` | Active for queue projection and element-model composition. |
| Work graph / Auto-DAG scheduling | `agent-doc-work-graph` | Active. Source-agnostic graph classification, schedule decision vocabulary, batch progress policy, and rendering live here so markdown documents, cross-document sources, and external PM adapters can feed the same dependency model. |
| Cross-cutting workflow kernel | `agent-doc-workflow` | Active. Pure evidence-to-decision-to-mutation/proof workflow policy for stale supervisors, queue drainability, captured responses, and live-buffer drift plus the machine-readable invariant catalog live here; orchestration keeps evidence gathering, effects, doctor/autofix adapters, and flow-event adapters. Serialization dependencies are allowed for the catalog; orchestration/git/editor IPC/sqlite/tmux dependencies are not. |
| Template-mode patch parsing, sanitization, repair, response materialization, and replay validation | `agent-doc-template` | Active. Pure patch parsing, patch content sanitization, patchback shape classification, orchestrate patchback contract policy, child template response normalization, response write-proof/materialization policy, replay-payload validation, component mutation, boundary repositioning, and duplicate prompt/tail repair live here; orchestration keeps file-backed config/document IO, IPC payload/file/log adapters, ops-log, and flow-event adapters. |
| Diff annotation and prompt-bearing classification | `agent-doc-diff` | Active. Pure comment stripping, unified-diff helpers, slash/preset/directive extraction, prompt-bearing change classification, unstarted prompt-bearing change selection, bare prompt-prefix target slicing, and partial-staging changed-literal/path-relatedness policy live here. |
| Frontmatter and project config parsing | `agent-doc-frontmatter` | Active. Document frontmatter schema, project config types, and pure parsing/serialization helpers live here; orchestration keeps file-backed IO in `project_config_io` and callers use that module directly rather than routing through `config`. |
| Filesystem helper primitives | `agent-doc-fs` | Extracted. Project-root discovery and optional file reads live here; orchestration and CLI callers use `agent_doc_fs` directly rather than routing through `snapshot` or an orchestration `fs_util` facade. |
| Editor visual syntax tokenization | `agent-doc-syntax` | Active. Pure visual token spans for editor integrations live here; FFI remains the ABI adapter. |
| Log timestamp formatting/parsing | `agent-doc-log-time` | Active. Pure log timestamp helpers live here; orchestration log writers/readers call it directly and must not re-export it through `ops_log`. |
| Lease/request TTL freshness | `agent-doc-lease` | Active. The shared saturating timestamp freshness rule for drain-owner, plugin-owner, queue-edit-owner, recycle-yield, and recycle-inflight sidecars lives here; domain modules call it directly and keep only their sidecar schema/path/env/effect adapters. |
| Exchange topic-section parsing | `agent-doc-topic` | Active. Pure `### Re:` exchange section splitting and boundary-tail stripping live here; compaction/archive code consumes the parser directly. |
| C/JNA editor ABI | `agent-doc-ffi` | Active. Pure C-ABI exports for editor integrations live here and depend on focused crates directly; the main crate remains the cdylib adapter. |
| Pure merge / conflict policy | `agent-doc-merge` | Active. Semantic merge, frontmatter-aware CRDT merge, CRDT state-vector sync, per-cell merge projection, merge write-ownership phases, events, transition table, liveness facts, and disk-write predicate live here; orchestration imports `agent_doc_merge::ownership` directly and only adapts plugin-owner/op-capture sidecar IO into that pure model. The deleted `merge_control_state_machine` facade must not return. |
| Turn lifecycle, operation log, and turn-scope affectedness | `agent-doc-turn` | Active. Lifecycle consumes realtime handoff proof and owns commit policy. Cycle phase transitions and labels, closeout guard reason labels and terminal outcome policy, append response heading normalization, imperative response contract classification, future-work response signals, closeout response text normalization, closeout prompt/response text matching, exchange-tail unresolved/prompt-only/response-heading policy, direct patchback heading classification, response/done-signal parsing, reaped queue-directive response-loss detection, free-text queue response-proof/residue detection, partial-closeout shipped/remaining-work detection, blocked follow-up closeout detection, gated-phase split body detection, queue-audit partial-completion collapse detection, closeout metadata-drift authoritative-side policy, closeout recovery mutation reason labels, closeout recovery state/action decision mapping, operation-log data types, turn-scope manifests, affectedness classification, and queue-continuation stall classification live here; orchestration imports `CyclePhase`, closeout guard/signal/recovery policy, exchange-tail policy, response text policy, heuristics, and drain-stall policy directly from `agent_doc_turn` and owns only sidecar persistence and command adapters. |
| Model tier and context usage | `agent-doc-model-tier` | Active. Harness model-tier resolution, harness alias canonicalization, preflight harness-mismatch warning facts, response-header model attribution, transcript token aggregation, Claude transcript path composition, model context-window lookup, Codex token-count percentage parsing, and the context-clear clear/no-clear diagnostic decision live here; orchestration keeps only JSON/file-backed transcript discovery/read adapters and calls the focused API directly. |
| Executor vocabulary | `agent-doc-turn-executor` | Active. Turn executor model vocabulary, managed capability-proof retry/timeout/status-message policy, auto-trigger readiness deadline policy, Codex resume launch argument policy, and idle-queue drain/context-clear readiness policy now live here; orchestration should call the focused API directly. |
| Tmux observations and command effects | `agent-doc-tmux`, `agent-doc-tmux-commands`, `agent-doc-tmux-io`, `agent-doc-turn-executor-tmux`, `tmux-router` | Active starter crates; orchestration still has transitional tmux code. Focus pane selection, pane-position geometry parsing/selection, and bare-shell foreground-command classification now live in `agent-doc-tmux`; harness submit profiles, submit diagnostic labels, submit-text normalization, and text/Enter command builders now live in `agent-doc-tmux-commands`; `focus`, `sessions`, and `route` only adapt file/session/probe/command-execution IO into those focused decisions. `sessions` no longer re-exports tmux-router or tmux-submit policy; callers import `tmux_router`, `agent_doc_tmux`, and `agent_doc_tmux_commands` directly. |
| SQLite-backed persistence | `agent-doc-sqlite` | Active. Actor storage record/state types and project-controller status/storage helper types live here and are imported directly; orchestration must not re-export them through `session_actor` or `project_controller`. |
| Document realtime scheduler | `agent-doc-document-realtime` | Active. Owns editor/disk read-authority policy, editor/disk authority states, apply scheduling states, CRDT authority policy, verification handoff vocabulary, pure live-editor delivery target selection, write/watch authority predicates, visible-write/source-proof/reconnect/editorless-disk write policy, and the inter-turn document convergence gate. |
| Editor debounce and sidecars | `agent-doc-debounce` | Active. Typing debounce state, live-buffer sidecars, editor sync barriers, write-provenance records, and live-buffer classification diagnostics live here; orchestration consumes the sidecar API directly. |
| Supervisor model and process effects | `agent-doc-supervisor`, `agent-doc-supervisor-process` | Active. Pure state/recovery decisions, supervisor config precedence, auto-install retry policy, stale install-artifact classification, stale host-supervisor binary identity checks, process lifecycle decisions, harness-change restart policy, child crash/restart policy, supervisor prompt/exit-code policy, restart-continuation policy, child-launch planning, idle-pane/ready-busy reconcile policy, route-owned reap and document-liveness policy, write-wedge recycle evidence classification, self-kill action/escalation/cmdline policy, post-child-exit run-loop dispatch, and controller handoff state now live in `agent-doc-supervisor`; process handoff state for `execve` re-entry now lives in `agent-doc-supervisor-process`. The old `start::decisions`, `supervisor::state`, `supervisor::resize`, and project-controller config helper facades are deleted; orchestration imports the focused crates directly. |
| Controller RPC/CAS | `agent-doc-controller` | Active. Controller stores/apply decisions; it does not own supervisor policy. Pure controller command-line recognition, cross-document owner command-line classification, controller status/freshness/control-plane projection, dispatch admission helpers for in-flight coalescing, authoritative-actor runtime guard classification, authoritative actor effective-state selection, dispatch-start proof classification, fresh-start ack outcome policy, routed cycle-ack requirement/missing-ack policy, routed reopen decision/action/prompt-ready-barrier policy, route submit-observation vocabulary/log rendering, routed trigger payload admission, route latency status/log rendering, route dispatch diagnostic message templates, duplicate-pane route diagnostic templates, route-dispatch bug backlog item templates, dispatch-only delivery/proof outcome vocabulary and log/refusal templates, routed reopen timeout budgets, direct-pane submit acceptance/outcome/dispatch-start-wait/poll-state/Enter-resubmit/resubmit-proof-log policy, dispatch-only starting-pane retry budgets, starting-timeout recovery classification, route closeout-block action selection, route startup-miss recovery classification, operator reopen classification, operator-clear guard policy, stale-generation redirect parsing, stale-queue pause recovery, dispatch-only busy/probe gates, route-drain retry classification, recycle debounce, force-recycle bypass, and claim cross-session admission now live in `agent-doc-controller`; orchestration imports them directly and the RPC/process-scan/claim/route layers remain only socket/process/tmux/file adapters that supply SQLite counts, process inode facts, bootstrap facts, duplicate-pid facts, and socket/process effects. |

## Core Split Targets

| Current module | Target crate | Notes |
|---|---|---|
| `component.rs` | `agent-doc-element::element` | Extracted. Element parsing is element/document syntax; keep parser pure and file-IO free. |
| `pending.rs` | `agent-doc-element-backlog` / `agent-doc-tracked-work` | Tracked work parsing and lifecycle should be shared by backlog, review, icebox, and done. Started: ordered tracked-work `#id` scanning lives in `agent-doc-element-backlog`; review-list projection/filtering and review ungate-task planning moved to `agent-doc-element-review`; orchestration keeps file-backed mutation adapters. |
| `queue_item_lifecycle.rs` | `agent-doc-element-queue` | Extracted. |
| `template.rs` / template patchback, sanitization, and response materialization policy | `agent-doc-template` | Extracted. Pure patch parsing, patch content sanitization, patchback shape classification, orchestrate patchback contract policy, child template response normalization, response write-proof/materialization policy, component mutation, boundary repositioning, and repair helpers live in `agent-doc-template`; file-backed config/document IO, IPC payload/file/log adapters, ops-log, and flow-event emission stay in orchestration adapters. |
| `replay_guard.rs` | `agent-doc-template` | Extracted. Replay payload shape validation lives beside template patch parsing because patch-bearing replay depends on `parse_patches`. |
| `crdt.rs`, `crdt_sync.rs`, `cell_doc.rs` | `agent-doc-merge` | Extracted. Pure CRDT/merge math, per-cell projection, and state-vector sync primitives live in `agent-doc-merge`; transport/authority adapters stay in orchestration/realtime layers. |
| `diff.rs` | `agent-doc-diff` | Extracted. Pure prompt-bearing classification, slash/preset/directive extraction, diff annotation, unstarted prompt-bearing change selection, bare prompt-prefix target slicing, and partial-staging changed-literal/path-relatedness policy live in `agent-doc-diff`; IO remains in orchestration adapters. |
| `frontmatter.rs` | `agent-doc-frontmatter` | Extracted together with pure project config parsing. File-backed wrappers stay effectful in orchestration adapters. |
| `op_log.rs`, `turn_scope.rs` | `agent-doc-turn` | Extracted. Operation-log data types, turn-scope manifests, and affectedness classification live with the turn lifecycle model; durable sidecar/sqlite IO stays in orchestration/sqlite adapters. |
| `heuristics.rs`, response text normalization, closeout response/done-signal parsing | `agent-doc-turn` | Extracted. Pending-capture recommendation detection, explicit no-follow-up response detection, future-work response signals, append response heading normalization, imperative response contract classification, closeout response text normalization, closeout prompt/response text matching, exchange-tail unresolved/prompt-only/response-heading policy, direct patchback heading classification, response/done-signal parsing, reaped queue-directive response-loss detection, free-text queue response-proof/residue detection, partial-closeout shipped/remaining-work detection, blocked follow-up closeout detection, gated-phase split body detection, and queue-audit partial-completion collapse detection are turn closeout policy; orchestration only applies the resulting signal to guards and file/cycle adapters. |
| `syntax.rs` | `agent-doc-syntax` | Extracted. Editor-facing visual tokenization is pure document syntax; FFI/editor integrations stay as adapters. |
| `topic.rs` | `agent-doc-topic` | Extracted. Exchange topic section parsing is pure text segmentation shared by compaction/archive flows. |
| `ffi.rs` | `agent-doc-ffi` | Extracted. C/JNA ABI exports depend on focused crates directly. |

`agent-doc-core` itself is deleted. Focused crates are the only Rust API
surface for extracted policy.

## Orchestration Split Targets

| Current area | Target crate | Notes |
|---|---|---|
| `write`, `compact`, editor convergence | `agent-doc-document-realtime` plus `agent-doc-merge` | Realtime scheduling, owner selection, delivery, and verification should leave orchestration. Live editor delivery target selection, write/watch authority predicates, visible-write idle/source-proof/full-content replacement policy, reconnect-buffer policy, editorless disk fallback policy, and the inter-turn convergence gate are pure in `agent-doc-document-realtime`; merge write-ownership is pure in `agent-doc-merge`; orchestration is only adapting current sidecar/editor/git/file IO into those decisions and emitting flow events. |
| Editor debounce, live-buffer sidecars, write provenance | `agent-doc-debounce` | Extracted. Cross-process typing markers, durable live-buffer snapshots, editor sync barrier polling, write-provenance sidecars, and live-buffer classification diagnostics live in the focused sidecar crate. |
| `preflight`, queue maintenance | `agent-doc-document`, `agent-doc-element-queue`, `agent-doc-queue`, `agent-doc-turn`, `agent-doc-work-graph`, `agent-doc-supervisor` | Preflight consumes parse/projection/supervisor facts; it should not own queue or supervisor semantics. Completed: queue item lifecycle moved to `agent-doc-element-queue`; id-backed queue directive target parsing, queue command/prompt classification, queue deletion identity/count comparison, queue continuation drainability/noise/context-reset policy, and guidance wording moved to `agent-doc-queue`; pure Auto-DAG classification/rendering, schedule decision vocabulary, and batch progress policy moved to `agent-doc-work-graph`; cycle phase transitions moved to `agent-doc-turn`; stale install-artifact classification moved to `agent-doc-supervisor`; orchestration keeps command/preflight/sidecar adapters. |
| Transcript context usage / clear source | `agent-doc-model-tier` plus orchestration `context_pct` adapters | Completed for pure transcript token aggregation, Claude project transcript path composition, model-window percentage, Codex `token_count` parsing, clear/no-clear diagnostic decision, preflight harness alias/mismatch facts, and agent-model attribution. Orchestration should only locate/read transcript files and adapt preflight JSON output around `agent_doc_model_tier`. |
| `start`, `supervisor`, stale actor reaping | `agent-doc-supervisor`, `agent-doc-supervisor-process`, `agent-doc-controller` | Distinguish pure supervisor state from process effects and controller storage. Completed: recycle/install/restart lifecycle policy, retry bounds, boot-resume policy, stale-drain yield policy, supervisor config precedence, agent-change restart policy, child crash/restart policy, supervisor prompt/exit-code policy, restart-continuation policy, child-launch planning, idle-pane and ready-busy reconcile policy, route-owned reap and document-liveness policy, write-wedge recycle evidence classification, self-kill action/escalation/cmdline policy, post-child-exit dispatch, and controller handoff policy moved to `agent-doc-supervisor`; `execve` re-entry handoff state and terminal resize effects moved to `agent-doc-supervisor-process`; the transitional `start::decisions`, `supervisor::state`, and `supervisor::resize` facades are gone, and orchestration keeps only idle-watch/process/tmux/document/file adapters. |
| Idle queue / executor readiness | `agent-doc-turn-executor` | Completed for pure idle queue drain, context-reset, clear-settle, dedup, between-turn enqueue planning, managed capability-proof retry/timeout/status-message policy, auto-trigger monitor/deadline policy, and Codex resume launch argument policy. Orchestration should only observe pane/editor/config facts and call the executor policy. |
| `route`, `project_controller`, `state_store`, `claim` | `agent-doc-controller` | Route and claim should become adapters to controller admission and actor bindings. Started: command-line recognition and cross-document owner command-line classification moved to `agent-doc-controller::command_line`; controller status/freshness/control-plane projection moved to `agent-doc-controller::status`; dispatch coalescing, authoritative-actor runtime guard classification, authoritative actor effective-state selection, dispatch-start proof classification, fresh-start ack outcome policy, routed cycle-ack requirement/missing-ack policy, routed reopen decision/action/prompt-ready-barrier policy, route submit-observation vocabulary/log rendering, routed trigger payload admission, route latency status/log rendering, route dispatch diagnostic message templates, duplicate-pane route diagnostic templates, route-dispatch bug backlog item templates, dispatch-only delivery/proof outcome vocabulary and log/refusal templates, routed reopen timeout budgets, direct-pane submit acceptance/outcome/dispatch-start-wait/poll-state/Enter-resubmit/resubmit-proof-log policy, dispatch-only starting-pane retry budgets, starting-timeout recovery classification, closeout-block action selection, startup-miss recovery classification, stale-generation retry parsing, stale-queue recovery, dispatch-only busy/probe gates, and route-drain retry classification moved to `agent-doc-controller::dispatch`; operator-clear guard policy moved to `agent-doc-controller::operator_clear`; controller recycle debounce and force bypass moved to `agent-doc-controller::recycle`; claim cross-session admission moved to `agent-doc-controller::claim`; orchestration still owns socket/process/tmux/file adapters and flow-specific UI outcome logging. |
| `tmux`, pane probing, command execution | `agent-doc-tmux-*`, `agent-doc-turn-executor-tmux` | Use pure command builders, typed observations, and focused pane-selection decisions. |
| `session_check`, closeout recovery classification | `agent-doc-turn` plus document realtime projections | Session check consumes lifecycle/realtime facts; it should not re-own source authority. Queue-continuation stall classification, closeout guard reason labels and terminal outcome policy, exchange-tail unresolved/prompt-only/response-heading policy, closeout response text normalization, response/done-signal parsing, reaped queue-directive response-loss detection, free-text queue response-proof/residue detection, partial-closeout shipped/remaining-work detection, blocked follow-up closeout detection, gated-phase split body detection, queue-audit partial-completion collapse detection, closeout metadata-drift authoritative-side policy, closeout recovery mutation reason labels, and closeout recovery state/action decision mapping are now in `agent-doc-turn`; unstarted prompt-bearing diff selection is in `agent-doc-diff`; orchestration persists the one-shot marker and adapts file/cycle state. |

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
