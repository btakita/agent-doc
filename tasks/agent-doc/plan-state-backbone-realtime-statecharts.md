# Plan: state backbone and realtime statechart consolidation

## Load-bearing document-turn contract (2026-07-16)

This section supersedes any older design that lets commands reconstruct turn
progress by reconciling capture JSON, cycle JSON, snapshots, ACK files, or live
buffer files. The entire document-turn pipeline is one durable state machine,
folded from facts in Lazily's SQLite-backed state ledger. A local statechart may
validate transitions, but it is not a second authority.

### Invariant

A turn may advance only monotonically from durable facts. `Committed` means the
same response intent is proven in the operator-visible document, through the
native editor-save boundary when an editor owns the document, and in the exact
Git commit. Operator edits are never replaced merely to make an agent transition
succeed. A deleted queue item is an operator edit and therefore cannot be
resurrected by replaying an older document image.

### Policy owner

`agent-doc-state-backbone` owns the event fold and durable transition state.
`agent-doc-turn` owns the pure transition function. Commands, supervisors,
plugins, document realtime code, and Git adapters submit evidence and execute
effects; none may independently choose or persist a lifecycle phase.

### State regions

The state is modeled as orthogonal regions so transport recovery cannot regress
document or commit progress:

| Region | Monotonic states |
|---|---|
| Response intent | `Absent -> Captured(response_hash, body_ref)` |
| Document projection | `Unobserved -> ApplyRequested -> Visible(authority_hash) -> NativeSaveProven(disk_hash)` |
| Git proof | `Unobserved -> CommitRequested(candidate_hash) -> Committed(head_sha, candidate_hash)` |
| Turn disposition | `Open -> TerminalConverged` or `Open -> OperatorBlocked` or `Open -> Abandoned` |
| Queue dispatch | `Stopped/Idle -> HeadAdmitted(head_id) -> HeadSettled(head_id)` |

`OperatorBlocked` is resumable after new evidence; it is not permission to pick
disk over the editor or the editor over disk. Terminal dispositions cannot be
silently exchanged. A conflicting terminal fact is an integrity error with both
facts retained for diagnosis.

### Transition table

| Current evidence | Event | Guard | Next fact/effect |
|---|---|---|---|
| response absent | `ResponseCaptured` | nonempty body + stable hash | persist body by hash and append captured fact |
| captured, not visible | `ProjectionObserved` | authority contains the exact response cell once | append visible proof; never replay a whole document |
| visible, disk differs | `NativeSaveObserved` | editor revision and saved hash correspond | append native-save proof |
| visible, authority=disk, lifecycle projection lags | `SessionCheckObserved` | response hash is exact in both | append missing write-applied proof monotonically |
| native-save proven | `CommitRequested` | candidate preserves current operator cells | execute exact Git commit |
| commit requested | `GitCommitObserved` | HEAD tree contains candidate hash | append commit proof and terminal convergence |
| any open state | `ReplicaLost` / timeout | no destructive choice is proven | request re-registration or emit operator block; retain intent |
| terminal converged | duplicate/reordered event | event id or proof already folded | idempotent no-op |
| any state | stale lower-rank projection | same cycle identity | ignore it; never regress |

### Evidence inputs

- Lazily document-cell identity, revision/frontier, and operator-edit events.
- Plugin receipt/applied/rejected and native-save observations.
- Exact authority, disk, response-cell, candidate-tree, and HEAD hashes.
- Replica membership/liveness facts and stable cycle, capture, delivery, and
  queue-head identities.

An immutable response body may be stored as a content-addressed payload referenced
by the ledger. That is blob storage, not a lifecycle sidecar: it has no phase and
cannot drive a transition by itself.

### Allowed edit surfaces

- Pure state/evidence types and reducers: `agent-doc-turn` and
  `agent-doc-state-backbone`.
- Evidence and effect adapters: the focused `*-io`, controller, supervisor, Git,
  and plugin crates.
- Lazily protocol bindings shared by Rust, Kotlin, and JavaScript.

The same event, state, guard, and effect names are used across these seams.
Adapters must not invent aliases such as `already_applied`, `write_applied`, and
`delivery_ack_pending` for the same fact without one typed conversion at the
boundary.

### Sidecar deletion sequence

1. Make the ledger projection the first and only transition read. Existing JSON
   is accepted solely by a one-way compatibility importer when the ledger lacks
   that historical cycle.
2. Stop every capture-state, cycle-state, ACK-content, snapshot-authority, and
   live-buffer-sidecar writer. Persist payload blobs and events through Lazily.
3. Delete compatibility readers after an instrumented release proves no imports
   occur. No dual-write parity window may make both stores authoritative.
4. Delete the sidecar directories, reapers, stale-sidecar repair commands, and
   doctor states that exist only for their reconciliation.

Routing is not an exception: editor membership and target identity come from the
Lazily replica/open-set projection, never from a filesystem lease.

### Verification

- Model-check every event ordering for monotonicity, idempotence, and terminal
  uniqueness; use generated traces as simulation-test fixtures.
- Run deterministic simulations with reordered/duplicated/dropped events,
  process and IDE restarts, held keys, unsaved cuts, concurrent operator deletes,
  replica loss, save delays, and Git failures.
- Assert operator cells are never removed or resurrected, a response cell lands
  at most once, no command recommends itself, and every recoverable state has one
  typed forward action.
- Keep focused reducer tests plus end-to-end Rust/Kotlin/JavaScript conformance
  tests for event names, hashes, and transition outcomes.

### Out of scope

- Electing disk or an old snapshot as authority when current operator intent is
  unknown.
- Treating a timeout as proof that an effect did not happen.
- Adding any new filesystem state file as a recovery shortcut.

## Theme

Agent-doc correctness-critical state should be consolidated into two places:

- Durable cross-process facts and projections live in `agent-doc-state-backbone`.
- Live document/route/write decisions live in focused realtime state charts or
  pure policy in `agent-doc-document-realtime` and adjacent focused policy
  crates.

IO crates may gather evidence, append/query project-control-pane facts, and wake
waiters. They should not own durable transition semantics through marker files,
ad hoc polling loops, or one-off sidecars when the state affects write, route,
commit, recycle, or closeout correctness.

## Placement Rules

- Use `agent-doc-state-backbone` for append-only facts that must survive process
  restart or be shared across controller, route, supervisor, editor, and commit
  paths.
- Use a lazily-backed projection in the project controller when callers need
  reactive live reads or waits over the PCP state.
- Use `agent-doc-document-realtime` for pure decisions about visible writes,
  editor authority, delivery proof, reconnect behavior, and commit-candidate
  proof.
- Keep IO adapters narrow: derive facts from files, editor IPC, git, debounce
  sidecars, or sockets, then call the backbone/realtime APIs.
- Marker files may remain only as advisory wakeups, compatibility read models,
  or inputs to fact derivation. They must not be the source of truth for
  correctness-critical state.
- Durable write proof is a bidirectional lazily projection: binary write-intent /
  expected response hash facts flow to the editor, and editor
  patch-received/applied/rejected / visible-buffer hash facts flow back. All
  maintained editor plugins must publish lazily receipt events and use those
  events directly. Legacy ACK/ack-content sidecars are not compatibility inputs;
  old ACK-only plugins must fail with a version/install error.

## Current Consolidation Targets

- Supervisor recycle in-flight state: move route direct dispatch and
  dispatch-only proof from marker polling to the PCP-backed supervisor recycle
  projection and lazily controller waiters.
- Supervisor recycle-yield state: move the self-draining loop yield gate from
  `.agent-doc/recycle-yield` marker files to controller recycle-yield helpers
  backed by the lazily supervisor recycle projection; keep only pure reason
  labels in `agent-doc-supervisor`.
- Route-submit in-flight state: replace `.agent-doc/route-in-flight` and
  `.agent-doc/route-submit-blocked` marker files with `RouteSubmitStarted`,
  `RouteSubmitSettled`, and `RouteSubmitBlocked` facts folded by the lazily
  route-submit projection; idle-watch reads the controller projection before
  queue drains or context-reset clears.
- Queue context-clear in-flight/deferred/cooldown state: replace
  `.agent-doc/context-clear-in-flight`, `.agent-doc/deferred-clears`, and
  `.agent-doc/queue-cooldowns` marker files with `QueueContextClearDeferred`,
  `QueueContextClearStarted`, and `QueueContextClearSettled` facts folded by the
  lazily-backed queue context-clear projection; idle-watch reads, promotes, and
  settles the controller projection before queue drains resume.
- Queue drain-stall continuation-pending state: replace the
  `.agent-doc/drain-stall/*` one-shot marker files with
  `QueueDrainStallContinuationRecorded` and
  `QueueDrainStallContinuationCleared` facts folded by the lazily queue
  drain-stall projection; session-check records it, preflight reconciles it,
  and idle-watch clears it when the supervisor itself progresses a drain.
- Document-model ensure state: preflight/current-document resolution must
  classify `editor_attached_model_missing` / `editor_sync_pending` as a
  recoverable bounded startup/reconciliation attempt before surfacing an
  agent-facing failure. The current relay hook asks the editor for a read-only
  `observe_lazily_current` and re-observes the relay; the remaining consolidation
  work is to fold ensure start/outcome and relay model usability into durable
  document backbone facts.
- Closeout lifecycle state: cycle-state transitions now append
  `PreflightStarted`, `ResponseCaptured`, `WriteApplied`, `CommitObserved`, and
  `CycleAbandoned` facts into the durable backbone with stable event ids. The
  git closeout path also appends exact `HEAD` SHA `CommitObserved` facts after
  real commits and already-current no-op closeouts. `session-check` and
  preflight now read the closeout projection before the legacy
  `state.db` closeout projection for open/terminal phase
  authority, and preflight fails closed when an open closeout projection is
  hidden behind missing or mismatched-stale JSON recovery state. The JSON
  projection still supplies detailed compatibility guard/recovery payloads. The
  remaining consolidation work is to make write/finalize, repair, and those
  detailed session-check/preflight guards read the closeout projection before
  consulting legacy recovery projections.
- Visible-write guard proof: replace broad live-buffer allowances with model
  revision, lazily receipt/applied/rejected events, editor-visible buffer hash,
  and commit-candidate hash facts folded by the backbone and decided by a
  realtime proof chart.
- Route dispatch proof: fold launch, dispatch-start, recycle, and receipt state into
  controller projections instead of repeated local sleeps and stale sidecar
  checks.
- Editor sync barrier: represent binary write intent, editor patch-received
  receipt, applied/rejected outcome, model revision, visible-buffer hash, and
  snapshot identity as projection facts before write/commit code uses them.
  ACK bodies are not compatibility authority.
- Commit guard: read durable commit-candidate proof first; keep legacy
  live-buffer heuristics only as temporary compatibility paths while they are
  converted into facts.

## Acceptance Criteria

- New correctness gates add typed `StateFact` variants and deterministic
  projections, or explicitly justify why the state is turn-local only.
- New realtime/write decisions have a pure policy function or state chart with
  unit tests in the focused policy crate.
- Route, write, git, supervisor, and controller IO crates import the focused
  policy/backbone APIs directly rather than recreating transition logic.
- Waits over shared state are reactive against the controller projection
  (`Condvar`, lazily cell, or subscription) with a deadline, not repeated marker
  polling in every caller.
- Static tests guard the crate boundary so orchestration does not regain
  facade ownership of the state model.
