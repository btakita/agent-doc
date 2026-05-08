# Deterministic Simulation Testing

`agent-doc` keeps targeted integration tests for real filesystem, git, tmux,
editor, and harness behavior. It also has a fast deterministic simulation layer
for workflow states that can be modeled in memory.

The simulator is not a second implementation of the CLI. It is a test-only
`SimWorld` that represents document text, snapshot text, cycle phase, captured
response body, fake commit outcome, backlog state, and boundary markers. Each
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
BindRouteOwner
SupervisorReady
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
```

Every generated failure must include a stable seed and command trace. The
normal fast corpus runs in `cargo test`; the wider deterministic corpus is an
ignored test that `make check` runs explicitly after the normal suite.

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
- Dispatch proof must match the same durable generation, session, and pane that
  accepted the dispatch.
- Stale actor generation updates, stale pane observations, and missing pane
  observations block dispatch/proof instead of silently creating duplicate
  authoritative owners or sending prompts to stale panes.

The simulator tests print schedule count, command count, elapsed time, and the
active budget for each corpus so CI logs show when generated coverage starts to
approach the budget.
