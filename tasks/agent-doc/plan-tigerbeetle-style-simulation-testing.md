# Plan - TigerBeetle-style deterministic simulation testing

**Pending:** `#tigertest`
**Status:** Proposed

## Problem

`agent-doc` has a large number of rare workflow bugs at boundaries where the
document, snapshot, git index, cycle state, pending list, IPC listener, tmux
state, and harness process can briefly disagree.

The current suite has many useful targeted regressions, but edge-case coverage
still grows mostly by adding one test per discovered failure. That catches known
bugs, but it does not efficiently explore combinations such as:

- user edits arriving between preflight and finalize
- response capture succeeding while commit or session-check fails
- pending mutations racing with template merge or backlog normalization
- stale pane/controller state overlapping with route/start recovery
- crash recovery resuming from partially written cycle files

The goal is to make these combinations cheap to generate, deterministic to
replay, and small enough to run frequently.

## Goal

Add a TigerBeetle-inspired simulation and property-test layer for `agent-doc`
that can execute many workflow schedules quickly while preserving exact repros
for every failure.

The intended result is not to copy TigerBeetle's storage-engine architecture.
The useful transfer is its testing posture:

- state transitions are explicit
- time, failures, and concurrency are injectable
- random schedules are deterministic from a seed
- failures emit enough trace data to become permanent regression tests
- most checks run against a fast in-memory model instead of real shells

## Non-Goals

- Do not replace existing integration tests that validate real git, filesystem,
  tmux, editor, or harness behavior.
- Do not make the simulator a second implementation of every `agent-doc`
  command.
- Do not require long-running randomized stress in normal developer loops.
- Do not force database-style static allocation constraints onto the CLI.

## Architecture

### 1. Build a pure `SimWorld`

Create a test-only harness that models the minimum state needed for closeout and
session correctness:

- document text and template components
- last snapshot text
- cycle-state phase and payload hashes
- git index/head/working-tree summary
- pending/backlog items and done/gated transitions
- controller/session actor facts when route/start behavior is in scope
- fake clock and deterministic random number generator
- fault points for filesystem, git, IPC, controller, and process boundaries

Where possible, call existing pure binary-owned code directly:

- component parsing
- template patch application
- diff classification
- planning-dispatch classification
- pending operations
- snapshot hashing
- cycle-state transition validation

Use fake adapters for impure boundaries first, then add a smaller set of real
integration schedules after the model catches useful bugs.

### 2. Define a command DSL

Represent generated schedules as explicit commands:

```text
EditDoc(kind)
Preflight
Plan
Finalize(response_kind, pending_ops)
WriteCommit(response_kind, pending_ops)
Repair
SessionCheck
RoutePrompt(prompt_kind)
ControllerTransition(event)
CrashAt(fault_point)
Recover
AssertInvariant(name)
```

Each command mutates `SimWorld` through the same production functions whenever
that is practical. The trace should be printable as a short textual repro that
can be copied into a focused regression test.

### 3. Assert workflow invariants

Start with invariants that map directly to recent bugs:

- Every prompt-bearing exchange line after the last committed boundary is either
  unresolved and blocks closeout, or has exactly one committed assistant
  response.
- A completed `do #id` directive leaves the matching backlog item done, gated,
  or explicitly open with a reason.
- `finalize`, `write --commit`, `repair`, and Codex Stop-hook replay cannot
  report success unless the session cycle reaches committed state and
  `session-check` would pass.
- A response captured in durable cycle state either lands in the document and is
  committed, or remains recoverable with a precise interruption reason.
- Template patch application never writes outside its target component, loses a
  user edit, or commits malformed component structure.
- Manual repo commits do not include the session document when the session
  closeout path still owns it.
- Snapshot and boundary state after recovery match the committed document that
  future preflight will diff from.
- Controller/session actor projections cannot become independent authority; any
  projection drift is diagnostic and recoverable from the durable actor state.

### 4. Make failures replayable and minimizable

Every failing generated schedule should print:

- seed
- command trace
- fault injection point
- document exchange tail
- snapshot hash and committed head summary
- cycle-state phase
- pending/backlog summary
- violated invariant and expected/actual facts

Add a shrink/minimize pass after the first version is useful. Until then, keep
the schedule language compact enough that manually reducing a failure is
practical.

### 5. Split fast and slow budgets

Run three tiers:

- `cargo test` fast corpus: fixed seeds plus focused generated schedules that
  complete in milliseconds.
- `make check` medium corpus: a wider deterministic seed range with fake
  filesystem/git/controller adapters.
- Optional local/nightly stress: larger seed ranges and selected real
  integration schedules with actual git temp repos and controller processes.

When a randomized failure is fixed, promote its minimized seed/trace into the
fast fixed corpus.

## Implementation Phases

### Phase 1 - Inventory pure seams

- List the production functions that can already be called without real IO.
- Identify closeout paths that still hide deterministic logic behind CLI-only
  wrappers and extract small pure transition functions where the extraction
  reduces test cost.
- Keep the extraction narrow; do not refactor route/start/controller code until
  a specific generated invariant needs it.

### Phase 2 - Closeout simulator MVP

Scope the first simulator to template documents and closeout:

- document edits
- preflight/plan classification
- response patch application
- pending add/done/gate operations
- cycle-state phases
- fake git commit success/failure
- session-check invariants

Target recent failure classes first: unresolved prompt after committed response,
malformed backlog prefixes, duplicate patchback, response-captured but
uncommitted, and snapshot/boundary drift.

### Phase 3 - Crash and fault injection

Add named fault points around:

- snapshot save
- fallback patch write
- IPC delivery
- template merge
- working-tree write
- index update
- git commit
- post-commit boundary reposition
- session-check

Each fault should have a deterministic expected outcome: automatic recovery,
explicit fail-closed interruption, or invariant-preserving no-op.

### Phase 4 - Route/controller schedules

After the controller/session actor shape stabilizes, extend `SimWorld` with:

- session actor generation
- supervisor lifecycle facts
- route dispatch acceptance/proof
- stale pane and missing pane observations
- projection drift between durable actor state and JSON/tmux projections

This phase should validate that route/start/sync cannot create duplicate
authoritative owners or silently dispatch to stale panes.

### Phase 5 - CI integration

- Add a stable fast-seed corpus to normal `cargo test`.
- Add a wider deterministic generated corpus to `make check` if runtime remains
  acceptable.
- Document how to run a seed range locally and how to promote a failing seed
  into a fixed regression.
- Track runtime budgets so generated coverage does not make the suite slow
  enough that developers avoid it.

## Candidate Files

- `src/agent-doc/src/component.rs`
- `src/agent-doc/src/template.rs`
- `src/agent-doc/src/diff.rs`
- `src/agent-doc/src/plan.rs`
- `src/agent-doc/src/pending.rs`
- `src/agent-doc/src/cycle_state.rs`
- `src/agent-doc/src/session_check.rs`
- `src/agent-doc/src/repair.rs`
- `src/agent-doc/src/finalize.rs`
- `src/agent-doc/tests/`

## Verification

- Unit tests for each simulator command and invariant.
- A fixed-seed closeout simulator test that runs at least hundreds of schedules
  in under a few seconds.
- Regression seeds for the known closeout/pending/snapshot failures listed
  above.
- A documented stress command for larger local seed ranges.
- Full `make check` once the first implementation lands.

## Exit Criteria

- A developer can reproduce any generated failure with one printed seed/trace.
- The fast test suite covers generated closeout schedules without requiring
  tmux, Codex, Claude, or editor plugins.
- At least three existing rare closeout bugs are represented as simulator
  invariants or fixed seeds.
- New simulator coverage has an explicit runtime budget and does not make
  normal `cargo test` materially slower.
