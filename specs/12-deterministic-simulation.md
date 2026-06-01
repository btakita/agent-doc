# Deterministic Simulation Testing

`agent-doc` keeps targeted integration tests for real filesystem, git, tmux,
editor, and harness behavior. It also has a fast deterministic simulation layer
for workflow states that can be modeled in memory.

The simulator is not a second implementation of the CLI. It is a test-only
`SimWorld` that represents document text, snapshot text, cycle phase, captured
response body, fake commit outcome, backlog state, boundary markers, a small
route/controller actor model, and sync-layout ownership projections. Each
command in a generated schedule mutates that world through production pure
functions whenever possible:

- `diff::unified_diff_from_contents` and `diff::classify_prompt_bearing_changes`
  classify unresolved prompt-bearing changes.
- `template::parse_patches`, `template::apply_patches`, and boundary
  reposition helpers apply response bodies and normalize exchange boundaries.
- `component::parse` scopes checks to live template components.
- `pending::detect_malformed_item_lines` verifies tracked backlog/icebox lines
  that would otherwise be hidden from pending guards.

The first supported command set covers closeout behavior:

```text
EditPrompt
EditLaterPrompt
AddMalformedBacklogItem
CaptureResponse
CaptureFallbackResponse
ApplyCapturedResponse
Commit
FailCommit
RepairBoundary
DuplicateVisibleResponse
CrashAt(FaultPoint)
Recover
SessionClear
BindRouteOwner
SupervisorReady
SupervisorBusy
SupervisorWaitingInput
SupervisorBlocked
SupervisorClosed
DispatchRoutePrompt
ProveDispatchAccepted
StaleSupervisorUpdate
ObserveStalePane
ObserveMissingPane
DriftProjection
RepairProjection
RepairBusyProjectionWithReadyPrompt
SyncProtectedGrowthManual
SyncProtectedGrowthPassive
SyncProtectedGrowthFocusVisible
SyncDetachableReplaceManual
SyncDetachableReplacePassive
SyncVisibleFocusPreserve
SyncRerequestVisibleEditorManual
SyncRerequestVisibleEditorPassive
```

Every generated failure must include a stable seed and command trace. The
normal fast corpus runs in `cargo test`; the wider deterministic corpus is an
ignored test that `make check` runs explicitly after the normal suite.

The default development suite must stay pure/fast. Tests that create an
`IsolatedTmux` server are live integration tests and must be marked ignored so
plain `cargo test`, `make test`, and `make check` do not pay tmux startup,
window reconciliation, shell readiness, or process-tree sampling costs. The
explicit `make tmux-ci` target owns the ignored live-tmux sweep and GitHub
Actions must run that target on Linux with `tmux` installed.

Tests that mutate or inspect process-global state (`std::env` or the process
current directory) must hold the reentrant shared test env lock from
`test_support::env_lock()` or a helper backed by that same lock, and mutations
must restore prior values through RAII. This keeps focused `cargo test <name>`
and unbounded `cargo test` runs from poisoning `session-check` regressions
while `make check` remains the supported full-suite entrypoint with
`TEST_THREADS=2`.

Current deterministic budgets:

- Fast corpus: seeds `0..512`, `24` commands per seed, budget `3s`, always run
  by `cargo test`.
- Medium corpus: seeds `0..2048`, `32` commands per seed, budget `12s`, run by
  `make check`.

To replay one seed while reducing a failure, run a focused test with the seed
mentioned in the panic trace and temporarily promote that schedule into a named
regression test in `src/sim_world.rs`. To validate the wider budget locally
without the full check suite, run:

```sh
cargo test closeout_sim_medium_seed_corpus_runs_wider_deterministic_budget -- --ignored
```

When a generated seed exposes a production bug, reduce the trace, add the
minimized command sequence as a focused regression, and keep the original seed
covered by the fixed fast corpus if it is cheap enough. If the fix needs a
larger generated range, adjust the corpus constants and this budget section in
the same change.

The closeout simulator also supports named Phase 3 fault points:

- `SnapshotSave`
- `FallbackPatchWrite`
- `IpcDelivery`
- `TemplateMerge`
- `WorkingTreeWrite`
- `IndexUpdate`
- `GitCommit`
- `PostCommitBoundaryReposition`
- `SessionCheck`

Fault outcomes are deterministic. Authoritative write/commit/session-check
faults fail closed with an interrupted phase and a trace; `Recover` must either
complete the closeout and align the snapshot with the committed document or
surface the remaining invariant violation. Non-authoritative IPC delivery
faults are modeled as invariant-preserving no-ops after the document and
snapshot are already committed.

Closeout invariants currently exercised by the simulator:

- A later unresolved `prompt_target` blocks closeout after a response write.
- A malformed tracked checklist line in backlog/icebox blocks closeout.
- A captured or write-applied response cannot be reported successful until it
  crosses the commit boundary.
- Duplicate visible response patchbacks are rejected before commit.
- Boundary cleanup leaves at most one live exchange boundary marker.
- A sync projection never presents the same document under two visible panes
  (no-duplicate-editor-pane cardinality invariant).
- IPC snapshot duplicate-prompt repair removes an extra live-typed prompt copy
  before the repaired response crosses the commit boundary.
- A focused post-exchange scratch-comment ownership fixture preserves visible
  ordinary comments through route-style cleanup, preflight-style recovery,
  direct write normalization, IPC/plugin snapshot handoff, repair-style
  write-closeout, and compact-style exchange replacement, while still proving
  generated duplicate comment residue without ownership proof is scrubbed
  line-by-line.
- Full-document replacement regressions cover late post-exchange scratch-comment
  edits: compact exchange direct write must reject stale whole-document
  replacements, and full-content IPC attempts must be disabled before socket/file
  payload emission.
- Baseline-drift replay coverage pins the three user-commit branches:
  benign edits outside the captured response auto-refresh replay hashes,
  edits inside the committed response fail closed, and user-normalized response
  bodies that still match the captured response after prompt-prefix stripping are
  adopted.
- Each named closeout fault point has an explicit fail-closed, recovery, or
  no-op outcome.

The simulator also covers route/controller schedules with a deliberately small
actor model:

- Durable actor state is the only authority for the active session actor
  generation, session id, pane id, and supervisor lifecycle.
- JSON/tmux-style projection state is a compatibility projection. Drift is
  diagnostic and must be repairable by copying from durable actor state; it
  must not become independent routing authority.
- Supervisor lifecycle facts can move through starting, ready, waiting-input,
  blocked, and closed states. Route dispatch is accepted only for the current
  ready generation.
- A JetBrains Clear Session Context simulation keeps `/clear` as an explicit
  session operator action while proving the following `Run Agent Doc` reroute
  still fails closed until the starting actor reaches a dispatch-ready prompt.
- Dispatch proof must match the same durable generation, session, and pane that
  accepted the dispatch.
- Stale actor generation updates, stale pane observations, and missing pane
  observations block dispatch/proof instead of silently creating duplicate
  authoritative owners or sending prompts to stale panes.

The simulator also owns pure sync-layout ownership schedules that do not need a
live tmux server:

- `sync_sim_tmuxbudget_seed_3001...` covers the open-closeout attach case:
  a hidden requested pane is brought into the visible projection even when the
  only unwanted visible pane owns an open closeout cycle. The open-cycle owner
  remains alive but can be stashed so sync does not create temporary visible
  pane growth.
- `sync_sim_tmuxbudget_seed_3002...` covers the replacement case: a hidden
  requested pane must replace an unprotected unwanted visible pane while a
  different open-cycle pane remains alive but detached from the visible
  projection.
- `sync_sim_tmuxbudget_seed_3003...` covers the focus proof: a preserve-layout
  return still reselects an already-visible requested focus pane.
- `sync_sim_tmuxbudget_seed_3004...` covers the remaining pure attach/focus
  variant: a hidden requested pane is attached while an open-cycle owner can be
  stashed and sync still focuses an already-visible requested sibling.
- `sync_sim_tmuxbudget_seed_3005...` covers the no-duplicate-editor-pane
  regression class ("3 tmux panes with 2 editor panes"): when the editor
  document is already visible and sync re-requests the same document — the
  duplicate-claim / pane-id churn surface logged as `duplicate_live_pane_claim`
  in `agent-doc-orchestration::sync` — the projection must keep a single editor
  pane rather than attaching a second one. The cardinality side of this is also
  enforced as a structural invariant on every command in the generated corpus
  (`SyncProjection.visible` must never list the same document twice).

This no-duplicate-editor-pane invariant is owned by the sync claim-tracking
(`claimed_sync_panes` / `SyncProofCache`) and the `tmux-router` per-column pane
dedup. It is independent of the `lazily-rs` dependency graph: the only
`lazily-rs` use on the sync path is `parse_frontmatter_for_sync`, which
constructs a fresh per-invocation `RunContext` (each slot computes at most once,
then drops), so no lazily-cached state survives across sync runs to drive
duplicate pane creation, and `tmux-router` does not depend on `lazily-rs` at
all.

Real tmux tests are still required in `make tmux-ci` for pane/window movement,
`tmux-router` reconcile behavior, shell/process ownership proof, and end-to-end
editor smokes. Pure ownership/cardinality variants should be promoted to named
SimWorld traces before the duplicate tmux-backed edge test is demoted from
local default coverage to the ignored live-tmux sweep.

The simulator tests print schedule count, command count, elapsed time, and the
active budget for each corpus so CI logs show when generated coverage starts to
approach the budget.

FlowCore also has a pure cross-flow regression that does not need the simulator
state machine: it proves routed dispatch-proof failure, editor-visible
typing/IPC write deferral, closeout `session-check` interruption, and queue
child plain-response patchback normalization all emit typed flow events that
`ops summary` can bucket. Promote new route/write/closeout/orchestration bug
classes into SimWorld only when the behavior needs schedule interleavings or
durable document state beyond those pure event contracts.
