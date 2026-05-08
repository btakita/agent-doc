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
```

Every generated failure must include a stable seed and command trace. Fixed
seed corpora run in normal `cargo test`; wider randomized or real-integration
stress may live in slower `make check` or local-only loops after runtime budgets
are known.

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

When a generated seed exposes a production bug, reduce the trace and promote it
to the fixed corpus before or with the bug fix.
