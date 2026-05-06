# Plan — evaluate a sqlite-backed session store for `agent-doc`

**Pending:** `#2dkn`
**Status:** Draft

## Problem

`agent-doc` is accumulating more durable coordination state than `sessions.json`
was originally meant to carry:

- live document -> pane/session bindings in `.agent-doc/sessions.json`
- authoritative actor state in `.agent-doc/session-actors.json`
- append-only session logs under `.agent-doc/logs/`
- cycle-state, startup-miss, pre-response, CRDT, baseline, and other sidecars

File-by-file locks have closed a number of direct clobber races, but the broader
concurrency model is still fragmented. Multi-step flows such as `start`,
`route`, `sync`, `repair`, closeout recovery, and editor-triggered passive sync
have to reconcile state spread across several files and projections. That
creates three structural limits:

1. Cross-file updates are not atomic.
2. Querying authoritative state requires heuristics plus sidecar inspection.
3. Parallel readers/writers scale by convention and flock discipline, not by a
   single transactional store.

The recent single-owner actor work improves ownership semantics, but it also
raises the bar for the storage layer: once one actor record becomes
authoritative, the project needs a clearer answer for compare-and-swap updates,
cross-process reads, and history/projection queries.

## Goal

Define an evidence-backed migration path for a sqlite-backed local session store
that can become the authoritative persistence layer for document-session state
without destabilizing current tmux/editor workflows.

The target outcome is not "replace JSON because sqlite sounds better." The
target outcome is:

- one transactional store for authoritative session/actor state
- explicit migration boundaries for existing projections and diagnostics
- proof that the sqlite layer reduces race surface and operational complexity
  instead of merely relocating it

## Non-Goals

- Do not rewrite the whole sidecar ecosystem in one pass.
- Do not block current bug-fix work on a large storage refactor.
- Do not require a background daemon or always-on broker just to read state.
- Do not treat sqlite adoption as mandatory if the investigation shows that the
  actor-store JSON model is still sufficient.

## Current Constraints To Respect

- `sessions.json` is still the read projection used across tmux routing,
  plugins, and recovery tools.
- `session-actors.json` is the durable actor-store direction already captured in
  `tasks/agent-doc/plan-single-owner-session-actor-transition.md`.
- `src/agent-doc/docs/reference/race-conditions.md` documents that several
  single-file clobber races are already mitigated with advisory locks.
- Mixed-root workspaces and sibling repos mean the store must remain project-root
  local and portable; no central service dependency is acceptable.
- Closeout and recovery flows need auditable, text-friendly diagnostics even if
  the authoritative state moves out of JSON.

## Why sqlite Is Worth Evaluating

sqlite materially changes the storage contract in ways the current file set does
not:

- single-writer / multi-reader transactional updates across related tables
- atomic compare-and-swap style ownership transitions
- indexed queries for live owner, last transition, pending closeout state, or
  session history without scanning multiple files
- easier invariant enforcement with unique constraints instead of best-effort
  dedup logic spread across callers
- clearer migration path for structured event history than ad hoc line-oriented
  logs plus projections

The cost is also real:

- schema and migration complexity
- more moving pieces in tests and local debugging
- possible duplication if projections remain JSON for too long
- risk of introducing an oversized subsystem before the actor contract settles

This plan should resolve that tradeoff with concrete checkpoints instead of a
one-shot rewrite.

## Proposed Target Model

Use sqlite as the authoritative local control plane for session coordination,
while keeping selected file outputs as projections during migration.

### Authoritative tables

- `documents`
  - canonical path
  - session id
  - harness
  - current generation
  - current actor state
  - current pane id / window id
  - last transition timestamp
- `actor_transitions`
  - document id
  - prior generation
  - new generation
  - caller
  - reason
  - old pane/window
  - new pane/window
  - timestamp
- `dispatch_attempts`
  - document id
  - generation
  - dispatch kind (`start`, `route`, `sync`, `restart`, `clear`)
  - acceptance / failure stage
  - stderr or diagnostic payload
  - timestamp
- `session_artifacts`
  - typed references to current sidecars that still live on disk
  - lets the binary correlate projections without making the artifact files
    themselves authoritative

### Projection outputs retained during migration

- `.agent-doc/sessions.json`
- `.agent-doc/session-actors.json`
- append-only text logs under `.agent-doc/logs/`

These become generated/read-through projections and diagnostics, not the source
of truth.

## Phased Plan

### Phase 1: prove the storage pain concretely

- Reproduce 2-3 recent bugs where the real issue was cross-file authority drift
  or non-atomic coordination rather than a single missing conditional.
- For each repro, document:
  - which files held relevant state
  - what lock/ordering assumptions existed
  - what would have been one transaction / one query in sqlite
- Confirm whether the current actor-store JSON path still needs multi-file
  compare-and-swap guarantees that JSON cannot express cleanly.

### Phase 2: define the minimal sqlite slice

- Add a narrow internal store module for:
  - document actor record load/store
  - monotonic generation transition
  - transition append
  - live owner lookup by canonical path
- Keep everything else on existing files.
- Prove that route/start/sync can read from the new store without changing user
  semantics yet.

### Phase 3: dual-write + projection verification

- Write authoritative actor/session updates to sqlite first.
- Emit `session-actors.json` and `sessions.json` as projections from sqlite in
  the same mutation path.
- Add drift checks that compare the sqlite row with the projected JSON content
  and fail closed on mismatch.
- Keep current logs, but attach stable row identifiers or generation ids so
  textual diagnostics map back to the store.

### Phase 4: move normal-path ownership queries to sqlite

- Route, sync, start, and session-status surfaces should read the authoritative
  owner from sqlite instead of inferring it from projections.
- JSON files remain compatibility outputs for plugins or debugging.
- Any remaining caller that still mutates JSON directly becomes a bug and should
  fail closed.

### Phase 5: decide how much other state belongs in the DB

- Evaluate whether startup-miss markers, cycle-state, and dispatch diagnostics
  should also move into sqlite, or remain sidecars keyed by authoritative row
  ids.
- Avoid a total absorption plan unless a given artifact benefits from
  transactional joins or indexed queries.

### Phase 6: retire JSON authority

- Once projections are proven stable, remove direct JSON authority semantics
  from `sessions.rs` and adjacent helpers.
- Keep JSON export only where editor compatibility or operator inspection still
  benefits from it.

## Design Questions To Resolve

1. Is sqlite needed before the single-owner actor transition lands fully, or
   should it start only after the actor contract is frozen?
2. Should the DB file be one project-root local `.agent-doc/state.db`, or
   split by purpose?
3. Which artifacts need durable event history in the DB versus remaining as
   human-readable append-only logs?
4. How should plugin-facing reads work during migration:
   - continue reading projected JSON only
   - add binary commands that query sqlite and keep plugins off raw files
5. What is the correct failure mode if projection write succeeds but sqlite
   commit fails, or vice versa?
6. Does WAL mode provide the right local read/write profile for concurrent CLI +
   editor activity, and are there portability concerns in the current dev
   environments?

## Implementation Sketch

- Add `src/agent-doc/src/store/` or equivalent module boundary around sqlite
  lifecycle, schema creation, and transaction helpers.
- Introduce a small `StoreTxn` API for:
  - `load_document(document_id)`
  - `record_transition(...)`
  - `compare_and_swap_generation(...)`
  - `append_dispatch_attempt(...)`
- Gate the first production use behind a feature flag or explicit migration mode
  so live sessions can opt in gradually.
- Add a read-only diagnostic command such as `agent-doc session status --json`
  that proves the new store can answer authoritative questions before it owns
  all mutations.

## Verification Strategy

- Unit tests for generation CAS semantics, uniqueness constraints, and
  transaction rollback.
- Regression tests for representative multi-step flows:
  - route to ready owner
  - fresh restart after stale owner
  - sync preserving layout while one document is unresolved
  - repair not reviving a superseded generation
- Migration tests that dual-write sqlite + JSON and assert projection equality.
- Crash-recovery tests that interrupt after sqlite commit but before projection
  write, and the inverse, to prove deterministic repair rules.

## Likely Files

- `src/agent-doc/src/session_actor.rs`
- `src/agent-doc/src/sessions.rs`
- `src/agent-doc/src/route.rs`
- `src/agent-doc/src/sync.rs`
- `src/agent-doc/src/start.rs`
- `src/agent-doc/src/resync.rs`
- `src/agent-doc/src/session_check.rs`
- `src/agent-doc/docs/reference/race-conditions.md`
- `tasks/agent-doc/plan-single-owner-session-actor-transition.md`

## Exit Criteria

1. The repo has a concrete decision record for whether sqlite should become the
   authoritative store for session coordination.
2. If the answer is yes, the first migration slice is small, testable, and
   compatible with existing projections.
3. The plan explicitly states which invariants move into transactional storage
   and which files remain derived diagnostics.
4. The work can be scheduled incrementally without blocking unrelated agent-doc
   bug fixes.
