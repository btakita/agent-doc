# Plan: track supervisor stale + recycle/restart state in lazily state

**Directive:** "Use lazily state to track the stale + restart state." Move the
recycle-**request** intent and the supervisor **staleness** observation out of ad-hoc
file markers / on-the-fly computation and into the `state.db` `StateFact` ledger (the
"lazily state" / Project Controller projection), matching the existing
`SupervisorRecycleStarted`/`SupervisorRecycleSettled` pattern
(`agent-doc-state-backbone/src/lib.rs`, whose doc-comment already says these facts
exist "instead of an on-disk recycle marker so route callers can wait on the lazily
state graph").

## Current state (what exists)

- **Recycle lifecycle (execve start→settle)** — ALREADY in lazily state:
  `StateFact::SupervisorRecycleStarted` / `SupervisorRecycleSettled` →
  `SupervisorRecycleProjection { phase: Settled|InFlight, reason, recycle_epoch,
  marked_secs }`. Emitted by the controller (`rpc.rs:supervisor_recycle_started_for_file`,
  epoch = `current.recycle_epoch + 1`).
- **Recycle REQUEST (pending intent)** — NOT in lazily state. File marker only:
  `agent-doc-supervisor-io/src/recycle_request.rs` (`request_recycle_for_doc`,
  `fresh_recycle_request`, `clear_recycle_request`). Callers: `rpc.rs:1531`, `5046`,
  `13104` (controller: admin recycle / install fan-out) and
  `write-converge-io/src/lib.rs:4622` (wedge-triggered request).
- **Staleness** — NOT in lazily state. Computed on-the-fly
  (`rpc.rs:host_supervisor_stale_warning_for_doc`, binary-mtime compare).
- **Wedge once-per-episode guard** (`#midturn-wedge-recycle`) — file marker
  (`recycle_attempted` flag on the dewedge marker, `write-converge-io`). Candidate to
  move into lazily state too.

## Target design

Add two fact families + project them into `SupervisorProjection`:

1. `StateFact::SupervisorRecycleRequested { document_hash, reason, recycle_epoch,
   marked_secs }` + `SupervisorRecyclePhase::Requested` + `SupervisorRecycleEvent::
   Requested`. Transition table (`transition_supervisor_recycle`): `Requested =>
   Requested`, `Started => InFlight`, `Settled => Settled`. Projection phase flows
   `Settled → Requested → InFlight → Settled`.
2. `StateFact::SupervisorStaleObserved { document_hash, installed_fingerprint,
   running_fingerprint, observed_secs }` + `SupervisorStaleCleared { document_hash,
   cleared_secs }` → a `stale: Option<SupervisorStaleProjection>` field on
   `SupervisorProjection`. Recorded when the controller detects staleness / when a
   recycle promotes the fresh binary.

## Exhaustive match sites for each new StateFact (compiler-guided)

`agent-doc-state-backbone/src/lib.rs`: `document_hash` extractor (~559), `StateDomain`
map (~601), `event_label` (~673), `apply` (~1484), and the fact-name structural guard
in `tests/test_cli.rs:9716`. Controller emitters in `rpc.rs` (~7509 for the epoch
increment). Reuse `supervisor_recycle_event_id`.

## Recording (who emits)

- **Requested:** new controller fn `supervisor_recycle_requested_for_file(file, reason)`
  paralleling `supervisor_recycle_started_for_file` (epoch = `current.recycle_epoch +
  1`, gated so it does not override an in-flight recycle). Call it from the three
  controller `request_recycle_for_doc` sites. The low-crate wedge caller
  (`write-converge-io:4622`) has no projection access — keep its file-marker write and
  let the controller reconcile it into a fact on its next tick (or thread a controller
  callback). Document this as the one intentional marker→fact bridge.
- **Consumed:** `clear_recycle_request` path → emit `SupervisorRecycleSettled` (or a
  dedicated `RecycleRequestConsumed`) so the projection returns to `Settled`.
- **Stale:** emit `SupervisorStaleObserved` where `host_supervisor_stale_warning_for_doc`
  first turns true; `SupervisorStaleCleared` after a successful recycle promotes the
  fresh binary (`binary_stale = false`).

## Readers (migrate off markers, projection-first with marker fallback)

- `idle_watch.rs:1710` `fresh_recycle_request` → read
  `projection.supervisor.recycle.phase == Requested` first, fall back to the marker
  during migration.
- `rpc.rs:13061-13111` recycle-request reads → projection-first.
- `supervisor_stale` in `idle_watch` → prefer `projection.supervisor.stale`.

## Tests

- `agent-doc-state-backbone`: `SupervisorRecycleRequested` → phase `Requested`; then
  `Started` → `InFlight`; then `Settled` → `Settled`; stale epoch-ordering; a stale
  (lower-epoch) `Requested` after `InFlight` is rejected.
- SimWorld: a requested-in-lazily-state recycle drives the same idle-watch decision as
  the marker did (parity), gated on the safe intra-turn checkpoint from
  `#midturn-wedge-recycle`.
- `test_cli.rs` fact-name guard updated.

## Risk

Durable-ledger change with fleet-wide blast radius via the projection's epoch gating.
Get `recycle_epoch` monotonicity right or a `Requested` fact is silently rejected as
stale (recycle never fires) or wrongly overrides an in-flight recycle. Validate epoch
ordering with the unit tests above before wiring readers off the markers; keep the
marker fallback until the projection path is proven live.
