# Plan: agent-doc FlowCore architecture refactor

## Purpose

Refactor agent-doc so recurring session, routing, patchback, and closeout bugs are handled by a small set of typed use-case flows instead of tactical fixes spread across large command modules.

The current pattern is visible in the logs and code shape: `route.rs`, `write.rs`, `git.rs`, `sync.rs`, `preflight.rs`, and `session_check.rs` each own part of the same lifecycle. That makes agents fix symptoms because there is no single place to reason about the flow contract.

## Evidence

Recent `agent-doc-bugs2.md` history shows the same failure classes under different names:

- Route/readiness bugs: starting actors, busy panes, accepted-only dispatch, stale or missing supervisor proof, and Clear Session Context racing active panes.
- Patchback/closeout bugs: zero-patch writes, late fallback writers after commit, response-not-committed repairs, malformed template patchbacks, duplicate scaffold, and stale snapshot drift.
- Prompt ownership bugs: missing or stripped `❯ ` prefixes, live-typing duplicate prompt prefixes, sidecar normalization fallback, and prefix/ownership loss across IPC ack content.
- Backlog and queue bugs: pending add order, cross-document pending add, backlog prompt orphan text, queue dispatch directive parsing, and pending capture guard failures.
- Orchestration bugs: preflight handoff cycle conflicts, mid-run task list mutation, child plain-response handling, and batch closeout interruptions.

The implementation shape supports the same conclusion. Approximate current line counts in the main hot path are:

- `route.rs`: 16.8k lines
- `write.rs`: 14.3k lines
- `sync.rs`: 11.4k lines
- `git.rs`: 7.6k lines
- `preflight.rs`: 5.7k lines
- `session_check.rs`: 4.8k lines

Large files are not automatically wrong, but here the same lifecycle concepts are duplicated across them: prompt ownership, document mutation, route proof, cycle state, commit state, snapshot state, and editor-visible state.

## Refactor Target

Introduce a `flow` layer that owns use-case state machines and typed outcomes. CLI commands should become thin adapters into these flows.

Proposed modules:

```text
src/agent-doc/src/flow/
  mod.rs
  types.rs
  session_cycle.rs
  routed_reopen.rs
  closeout.rs
  document_mutation.rs
  operator_clear.rs
  orchestration_batch.rs
  proof.rs
```

### Shared Types

Create one vocabulary shared by CLI, route, write, git, session-check, orchestration, and editor IPC:

- `FlowId`, `CycleId`, `DocumentId`, `ActorGeneration`
- `PromptOwnership`: user, assistant, system, structural
- `DocumentMutationKind`: exchange_patch, backlog_op, icebox_patch, full_content, repair
- `PatchbackShape`: valid_patch, plain_response, malformed_patch, transcript_dump, mixed_output
- `RouteDecision`: reuse_ready, wait_for_ready, fresh_restart, start_new, fail_closed
- `DispatchProof`: accepted_only, dispatch_started, consumed, timed_out, rejected
- `CloseoutState`: preflight_started, response_captured, write_applied, committed, abandoned
- `FlowOutcome`: completed, blocked, failed_closed, externally_blocked

All high-risk branches should return these types instead of free-form strings plus later log parsing.

## Flow Contracts

### `session_cycle`

Owns the normal direct invocation:

```text
preflight -> plan -> execute requested work -> patchback -> commit -> session-check
```

The flow must decide:

- what prompt-bearing changes exist
- whether the cycle is a no-op, repair, normal response, or handoff
- whether pending/backlog capture is required before closeout
- which baseline is authoritative
- whether terminal success means response visible in the document and committed

### `routed_reopen`

Owns same-document route decisions:

```text
classify actor binding -> prove prompt readiness -> submit -> prove dispatch-start -> report
```

The route path should not also be a repair engine. It should choose one bounded recovery action or fail closed with one typed reason.

### `closeout`

Owns response persistence:

```text
parse response -> apply document mutation -> converge editor/disk/snapshot -> commit -> verify guard
```

Late fallback writers, IPC socket writes, direct disk writes, and repair writes should all pass through the same terminal-state guard.

### `document_mutation`

Owns template/CRDT/full-content mutation semantics:

- parse and validate patch shape before editor-visible mutation
- keep prompt ownership normalization in one path
- run duplicate-scaffold and duplicate-prompt repair before any ack-content snapshot is trusted
- assert post-write structure before commit

### `operator_clear`

Owns Clear Session Context and interrupt-clear:

- allow clear only after idle prompt or clean-exit proof
- refuse active `agent-doc` panes with typed UX data
- model interrupt-clear as an explicit destructive flow, not a route side effect

### `orchestration_batch`

Owns queue/orchestrate batch invariants:

- freeze batch at start
- detect mid-run edits after each child closeout
- normalize child output through the same patchback contract
- record each child result as structured proof

## Implementation Phases

### Phase 0: inventory and flow map

Create a matrix of the hot-path states and the modules currently mutating them.

Deliverables:

- `docs/reference/flow-map.md` or `tasks/agent-doc/flow-map.md`
- list of duplicated state checks and log strings
- list of stringly typed proof fields that should become enums

Acceptance:

- every recurring bug class above maps to exactly one proposed owning flow
- no code behavior changes yet

### Phase 1: add FlowCore types and mirror-mode instrumentation

Add `flow::types` and `flow::proof` with typed events and outcomes. Initially, existing code emits mirrored `FlowEvent` records without changing behavior.

Acceptance:

- existing `make check` passes
- ops summary can group route/write/commit failures by typed flow stage
- no command behavior changes

### Phase 2: extract routed reopen

Move pure route classification out of `route.rs` into `flow::routed_reopen`. Keep tmux and supervisor side effects behind small ports.

Acceptance:

- route decision tests can run without tmux
- same-document route has one state machine and one retry budget
- `route.rs` shrinks by removing decision branches, not by moving text wholesale

### Phase 3: extract closeout and document mutation

Move patch parsing, prompt ownership normalization, duplicate repair, snapshot convergence, and terminal-state guards into `flow::closeout` and `flow::document_mutation`.

Acceptance:

- IPC, stream, direct write, finalize, and repair all use the same terminal-state guard
- late fallback after committed cycle is impossible by construction
- duplicate scaffold and prompt-prefix regressions live beside the mutation flow

### Phase 4: extract session cycle

Make `preflight`, `plan`, `finalize`, `write --commit`, and `session-check` adapters over `flow::session_cycle`.

Acceptance:

- `agent-doc <FILE>` and direct `finalize` use one closeout contract
- preflight cannot commit away a new prompt before diff classification
- pending/backlog capture requirements are represented in the typed cycle outcome

### Phase 5: extract operator clear and orchestration batch

Move Clear Session Context and orchestrate/queue behavior onto the shared flow types.

Acceptance:

- Clear Session Context cannot send input to an active `agent-doc` pane without explicit destructive confirmation
- orchestration child outputs use the same patchback parser and terminal closeout guard as normal responses

## Testing Strategy

Use three layers:

- Pure unit tests for flow decisions and state transitions.
- SimWorld schedules for concurrency: startup races, stale writers, live typing, snapshot drift, and route/clear interleavings.
- One small tmux smoke per external boundary: input dispatch, prompt readiness, and editor-visible IPC convergence.

The goal is not more tests around tactical branches. The goal is to make the branch count small enough that each recurring class has one owning state machine and one deterministic regression location.

## First Backlog Items

1. Implement Phase 0 and Phase 1 first. Do not start by rewriting `route.rs`.
2. Extract routed reopen second because it is the most user-visible recurrence.
3. Extract closeout/document mutation third because it is the highest data-loss risk.
4. Only then move orchestration and Clear Session Context onto the same flow contracts.
