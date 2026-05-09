# Plan: Controller singleton with zero-downtime handoff

## Problem

The project controller currently has a launch lock and binary identity check, but
the replacement path still has two unsafe edges:

1. A newer client can shut down a stale controller before a replacement is ready,
   leaving a window where route/start/sync clients cannot reach any controller.
2. Shutdown only reaches the controller currently bound to
   `.agent-doc/controller.sock`; duplicate same-project controller processes that
   no longer own the socket can remain alive and keep stale state or supervisor
   relationships.

The required invariant is stronger: for one project root there must be exactly
one authoritative controller generation accepting new requests, and binary
replacement must preserve active session actors and supervisors without an
observable outage.

## Target Behavior

- `agent-doc controller status --ensure` either connects to the current
  authoritative controller or completes a controlled handoff to a replacement.
- New requests are never routed to two controller generations at the same time.
- During replacement, existing session actor records, supervisor leases,
  dispatch attempts, and projection diagnostics remain durable in
  `.agent-doc/state.db`.
- The old controller exits gracefully after it has stopped accepting new
  authority-changing requests. If it cannot exit, the new controller reports and
  reaps it by PID after the handoff is complete.
- A failed replacement keeps the old controller authoritative when it is still
  healthy, rather than leaving the project without a controller.

## Design

### 1. Persist controller generation ownership

Extend `.agent-doc/controller-state.json` or the SQLite store with:

- `controller_generation`
- `controller_pid`
- `controller_binary`
- `handoff_state`: `stable`, `preparing`, `promoted`, `retiring`, `failed`
- `handoff_started_at`
- `previous_controller_pid`

The launch lock protects updates to this generation record. The socket path is
only the transport endpoint; it must not be the sole source of singleton truth.

### 2. Add an explicit handoff protocol

Add controller RPCs:

- `prepare_handoff { new_binary, new_pid }`
- `export_runtime_state`
- `promote_handoff { generation }`
- `retire_after_handoff { generation }`
- `reap_duplicate { pid, generation }`

The replacement flow should be:

1. New client acquires the launch lock.
2. If the bound controller is healthy but stale, ask it to
   `prepare_handoff`.
3. Launch the new controller on a temporary socket.
4. New controller imports durable state from SQLite plus any live runtime facts
   exported by the old controller.
5. New controller marks itself `promoted` in the generation record.
6. Atomically switch the public socket endpoint to the promoted controller.
7. Ask the old controller to retire.
8. If the old PID remains alive after a bounded grace period, kill only that
   verified same-project controller PID and record a diagnostic.

If steps 3-5 fail, the old controller clears `handoff_state` and remains the
public authority.

### 3. Keep active supervisors connected through stable identity

Supervisor leases should be durable enough that a promoted controller can
continue to answer `session_status`, `route`, and `sync` immediately after
promotion. Supervisors should reconnect on their next heartbeat when the
controller generation changes.

For the first implementation slice, the handoff can avoid live stream transfer:
the lease record plus supervisor socket path is enough for the new controller to
recover dispatch/status authority. Any in-flight dispatch must finish under the
old controller before retirement or be marked `interrupted_by_handoff` with a
precise diagnostic.

### 4. Detect and reap duplicate controllers

Add same-project controller discovery:

- start with `controller-state.json` PID and `previous_controller_pid`
- scan process command lines for `agent-doc controller serve --project-root
  <canonical-root>`
- verify each candidate by binary identity and project root before acting

`status --ensure` should warn or fail closed when multiple candidates are found
and no safe authority can be proven. After a successful promotion, it may reap
non-authoritative same-project controllers.

### 5. Update shutdown semantics

`agent-doc controller shutdown` should default to graceful retirement of the
authoritative controller generation. Add a repair-only force mode for duplicate
cleanup, but keep normal shutdown from killing unrelated `agent-doc` processes.

## Implementation Steps

1. Add generation/handoff fields to controller bootstrap state and SQLite
   schema, with migration/default handling for existing projects.
2. Split `connect_or_launch` into `connect_authoritative_or_handoff` so stale
   binary replacement goes through the handoff protocol instead of
   `shutdown_stale_controller`.
3. Add temporary-socket controller launch support and promotion to the public
   socket only after the new controller reports ready.
4. Add duplicate same-project process discovery and safe reap diagnostics.
5. Make supervisor heartbeat/reconnect include observed controller generation.
6. Update `controller status --ensure`, `controller shutdown`, and `doctor` to
   surface generation, handoff state, stale duplicate PIDs, and repair commands.

## Tests

- Unit: launch lock plus generation record admits only one promoted controller.
- Unit: stale binary replacement keeps the old controller stable when new launch
  fails before promotion.
- Unit: successful handoff promotes a new generation and retires the old PID.
- Unit: duplicate process scan only matches `agent-doc controller serve` for the
  same canonical project root.
- IPC integration: while handoff is preparing, concurrent `status`,
  `session_status`, and `sync --no-autostart` requests either complete against
  the old generation or retry against the promoted generation without timing out.
- Supervisor integration: a ready supervisor lease survives controller
  promotion and the next heartbeat binds to the new generation.
- Regression: the observed stale duplicate same-project controller shape is
  diagnosed and reaped after a successful promotion.

## Validation

Run focused controller tests first:

```bash
cargo test -p agent-doc project_controller -- --nocapture
cargo test -p agent-doc supervisor -- --nocapture
```

Then run the full required suite:

```bash
make check
make build-release install
agent-doc --version
```

## Exit Criteria

1. A healthy project never has two promoted authoritative controllers.
2. Binary replacement has no gap where the public controller endpoint is absent
   unless no old controller was healthy enough to preserve.
3. Active session actors and supervisor leases survive controller promotion.
4. Duplicate same-project controller processes are either retired gracefully or
   force-reaped after identity verification.
5. Status/doctor output explains controller generation, handoff state, and any
   stale duplicate PIDs.
