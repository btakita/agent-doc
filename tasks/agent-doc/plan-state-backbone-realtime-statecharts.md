# Plan: state backbone and realtime statechart consolidation

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
  `publish_live_buffer` and re-observes the relay; the remaining consolidation
  work is to fold ensure start/outcome and relay model usability into durable
  document backbone facts.
- Closeout lifecycle state: cycle-state transitions now append
  `PreflightStarted`, `ResponseCaptured`, `WriteApplied`, `CommitObserved`, and
  `CycleAbandoned` facts into the durable backbone with stable event ids. The
  git closeout path also appends exact `HEAD` SHA `CommitObserved` facts after
  real commits and already-current no-op closeouts. `session-check` and
  preflight now read the closeout projection before the legacy
  `.agent-doc/state/cycles` recovery projection for open/terminal phase
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
