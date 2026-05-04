> Supplement to [08-session-routing.md](08-session-routing.md)

# Session Actor Contract

This file freezes the phase-1 single-owner session actor contract. It does not
introduce the durable actor store yet; it defines the semantics that later
phases must preserve.

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

- `.agent-doc/session-actors.json` is the durable per-document actor record
  store. It is keyed by canonical document path and carries the authoritative
  generation, pane/window binding, harness, state, and last transition.
- Store updates must be monotonic and fail closed on generation regressions; a
  stale writer must not overwrite a newer generation.
- Same-generation state transitions must also fail closed when the caller's
  `session_id` or `pane_id` no longer matches the authoritative record.
- The phase-3 supervisor path reports these actor transitions explicitly:
  `prompt_ready`, `ipc_inject` / `auto_trigger_inject` busy dispatch, clean-exit
  `waiting_input`, `supervisor_halted`, and final `closed`.
- The phase-4 route path now consumes that authoritative actor record directly
  when the supervisor is healthy: it dispatches the bare reopen through
  supervisor IPC to the actor-owned pane, waits for the normal routed
  acknowledgment window, and only refreshes `sessions.json` as a projection of
  the actor binding.
- `sessions.json` remains a projection/binding helper during migration, not the
  final actor store.
- Session logs are the source of transition provenance and generation history.
- Startup-miss, route, sync, and closeout recovery may read those logs, but
  they must not infer authority from multiple competing mutable sources once a
  newer generation is recorded.
