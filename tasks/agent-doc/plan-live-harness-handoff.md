# Plan — Live supervisor harness handoff (`#harnesshotrebind`)

## Architecture contract

Invariant: an explicit frontmatter harness change must never dispatch into the
old harness and must not require a manual supervisor restart; a healthy live
supervisor accepts the route as a boundary handoff, drains any active turn,
fresh-spawns the resolved harness, and auto-triggers the document exactly once.

Policy owner: `agent-doc-supervisor::agent_change`. Route adapters gather the
normalized actor/frontmatter identities, restart knob, queue state, supervisor
health, actor state, and restart-in-flight evidence, then apply the typed
decision without independently deciding whether a live actor may be replaced.

Transition table:

| Input | Decision | Effect |
| --- | --- | --- |
| Harnesses match | Normal route | Existing dispatch path is unchanged. |
| Mismatch, old actor stale/closed | Normal replacement | Existing create/rebind path owns recovery. |
| Mismatch, healthy live actor, restart enabled | Accept boundary handoff | Return routed success without injecting the old pane; idle watch switches at the next safe boundary. |
| Same handoff already restarting | Coalesce | Return the same accepted handoff; do not interrupt or duplicate-trigger. |
| Active old-harness turn | Accept and wait | Preserve the turn; switch and auto-trigger after it reaches the boundary. |
| Queue paused | Hold for resume | Preserve the accepted handoff and surface resume as the exact prerequisite; do not require restart. |
| Restart disabled | Block | Fail explicitly; no false promise that a handoff will occur. |
| Supervisor unhealthy | Normal replacement/fail-closed | Existing stale-authority policy remains authoritative. |

Evidence inputs: normalized persisted and resolved harness names, an explicit
frontmatter declaration, supervisor runtime health/state, queue-control state,
the restart-enable knob, and the last transition reason.

Allowed edit surfaces:

- `agent-doc-supervisor/src/agent_change.rs` for the exhaustive policy.
- `agent-doc-route-io` for typed pending-handoff transport and the no-injection
  success adapter.
- `src/sim_world*` for end-to-end transition coverage.
- specs/version notes and focused editor wording only where the operator
  contract changes.

Verification: pure policy truth-table tests; route target tests proving a
pending handoff is never dispatch-eligible; SimWorld proofs for ready, busy,
paused, disabled, and coalesced paths; focused crate tests; full `make check`
and `make tmux-ci`.

Out of scope: switching a harness in the middle of an active model turn,
cross-harness conversation-history migration, or adding a second supervisor.

## Outcome

Implemented in 0.35.17. The typed policy, non-dispatchable route target, live
IPC harness projection, stale-record reconciliation, route/SimWorld regression
matrix, `make check`, `make tmux-ci`, and release-parity install all pass. The
JetBrains 0.2.286 distribution is unchanged and installed from the verified
canonical ZIP.
