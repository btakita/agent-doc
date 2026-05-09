> Supplement to [08-session-routing.md](08-session-routing.md)

# Session Actor Contract

This file freezes the phase-1 single-owner session actor contract while
documenting the phase-2 durable actor store boundary so later phases preserve
the same semantics.

## Authoritative intent

The normal path should behave as if one per-document session actor owns
authority, even while implementation still spans registry entries, supervisor
state, and session logs.

Authoritative fields:

- `document_id`
- `session_id`
- `generation`
- `pane_id`
- `window_id`
- `harness`
- `state`
- `last_transition`

## Generation semantics

- Generations are monotonic per document session and start at `1`.
- A new generation is created whenever normal-path control explicitly starts a
  new document session owner or rebinds authority to another pane.
- Harness child restarts inside the same owning supervisor do not create a new
  ownership generation by themselves.
- Same-generation lifecycle updates such as prompt readiness, dispatch-busy,
  waiting-for-input, blocked, and closed must preserve the authoritative
  generation while still updating `state` and `last_transition`.
- Legacy logs without explicit generation markers may infer historical
  generations from repeated `session_start` events for diagnostics, but new
  writes must emit explicit generation metadata.

## Ownership transition logging

Every ownership transition must emit a machine-readable log line with:

- `caller`
- `reason`
- `prior_generation`
- `new_generation`
- `old_pane`
- `new_pane`
- `old_window`
- `new_window`

Canonical event shape:

```text
ownership_transition caller=<caller> reason=<reason> prior_generation=<n> new_generation=<n+1> old_pane=<pane|none> new_pane=<pane> old_window=<window|none> new_window=<window|unknown>
```

Phase-1 callers:

- `start`
- `claim`
- `route`
- `sync`
- `register`

The stable requirement is the field schema, not the complete caller taxonomy.
Later phases may refine caller values without changing the field names.

## Current phase boundary

- `.agent-doc/state.db` is the durable per-project controller state store for
  actor records. The `documents` table carries the authoritative generation,
  pane/window binding, harness, state, and last transition id, while
  `actor_transitions` records each monotonic ownership or lifecycle transition.
- `.agent-doc/session-actors.json` is now a projection emitted from committed
  SQLite state for compatibility. Route/start/sync must ask the project
  controller for the actor binding before consulting legacy compatibility
  evidence; the JSON projection must not become an independent write authority
  again.
- Actor-record `harness` values use canonical ids rather than raw binary names:
  `claude` normalizes to `claude-code`, `codex` stays `codex`, and empty values
  collapse to `default`. Normal-path route/start/sync checks must compare
  against that canonical identity so harness aliases do not strand a healthy
  authoritative actor.
- Store updates must be monotonic and fail closed on generation regressions; a
  stale writer must not overwrite a newer generation.
- Same-generation state transitions must also fail closed when the caller's
  `session_id` or `pane_id` no longer matches the authoritative record.
- Controller IPC must be resilient to stalled peers. Server-side request reads
  and client-side response reads are bounded, each accepted client is handled
  independently, and readiness checks must release any idle controller stream
  before issuing a real RPC.
- Controller lazy launch must not trust a stale `current_exe()` path after a
  local binary replacement. If that path no longer exists, launch and bootstrap
  identity resolution fall back to the invoked command or `agent-doc` on `PATH`;
  only then may the client fail closed with the missing path in the diagnostic.
- The phase-3 supervisor path reports these actor transitions through the
  project controller IPC, not by independently rewriting actor files:
  `start_session`, `register_supervisor`, `prompt_ready`, `ipc_inject` /
  `auto_trigger_inject` busy dispatch, clean-exit `waiting_input`,
  `supervisor_halted`, and final `closed`.
- Lifecycle updates must include the caller's session id, pane id, and
  generation. The controller rejects stale lifecycle reports when any of those
  fields no longer match the authoritative actor record, and it updates
  `supervisor_leases` with the latest runtime state while preserving the
  registered supervisor pid/socket across state-only reports.
- `prompt_visible_once` remains a child-lifecycle fact for restart heuristics, but
  prompt-driven `busy -> ready` recovery must fire after every later routed or
  auto-trigger dispatch that returns the same child to an idle prompt; it is not
  a spawn-only one-shot transition.
- The phase-4 route path now consumes the authoritative actor binding through
  project controller IPC. Before submitting a managed or dispatch-only reopen
  into an actor-owned pane, route records a controller `dispatch` attempt for
  the current session id, pane id, generation, and command kind. Stale
  session/pane/generation requests fail closed before pane input is submitted,
  while accepted dispatches record the stage (`ready`, `starting_queue`,
  `busy_queue`, or `waiting_input_recovery`) in `dispatch_attempts`.
- The phase-5 sync/focus path also consumes that authoritative actor binding
  through the controller when the actor pane is still alive: sync
  rescues/reconciles layout around the actor-owned pane and focus selects it
  before falling back to supervisor-backed registry compatibility evidence.
- The phase-6 operator surface is actor-backed as well: `session status`,
  `history`, `attach`, `restart`, `clear`, and `doctor` read or mutate the same
  authoritative record and supervisor IPC path instead of inventing separate
  tmux-only heuristics in the CLI or plugins.
- Operator commands must use the project controller as their actor boundary:
  status/history read controller-owned SQLite rows for the actor, transitions,
  supervisor lease, recent command attempts, and projection diagnostics; attach
  creates the manual handoff generation through controller IPC before refreshing
  `sessions.json` as a projection; restart and clear record an
  `operator_<state>` acceptance or stage-specific rejection before touching the
  supervisor socket or tmux input.
- Operator diagnostics must preserve the failed stage. Missing actors,
  blocked/closed actor states, stale projections, and recent failed operator
  attempts are durable controller diagnostics that editor plugins can display
  without scraping tmux state.
- The phase-7 repair boundary is now explicit: normal `sync` may capture
  diagnostics and fail closed on stash/window or closeout drift, but it does
  not run hidden layout rescue or closeout replay anymore. Those mutations live
  behind explicit repair surfaces such as `agent-doc repair <FILE>` and
  `agent-doc session doctor <FILE> --repair`.
- The phase-8 normal-path cleanup is now in place: route/start/sync no longer
  re-elect ownership from latest-open session logs, `registry_rebind`
  successors, or generic same-file process-tree scans. Those sources still
  matter for diagnostics and explicit repair, but they cannot silently reclaim
  authority away from the actor-backed path.
- Phase-9 verification must keep explicit regression coverage for generation
  monotonicity; phase-4 authoritative route-state handling (`ready` direct
  dispatch, `starting` / `busy` optimistic queueing, `waiting_input`
  fresh-restart recovery, and `blocked` / `closed` fail-closed rejection);
  stale session/pane rejection; blocked mixed-root layout preservation; and
  plugin operator surfaces that display exact session status, route
  `session clear` through the actor-backed command path, and preserve
  stage-specific dispatch failures in a durable diagnostics surface.
- `sessions.json` remains a projection/binding helper during migration, not the
  final actor store. Controller-backed actor writes reconcile existing registry
  entries to the actor binding; if either `session-actors.json` or
  `sessions.json` cannot be emitted or drifts from SQLite, the actor state stays
  authoritative and the controller records projection diagnostics.
- Session logs and `ops.log` are transition provenance outputs. The controller
  persists actor state first, then emits projection/log diagnostics from that
  committed state.
- Startup-miss, route, sync, and closeout recovery may read those logs, but
  they must not infer authority from multiple competing mutable sources once a
  newer generation is recorded.
