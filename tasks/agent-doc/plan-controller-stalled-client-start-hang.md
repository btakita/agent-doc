# Plan: fail closed when a stalled controller client blocks `agent-doc start`

## Problem

`agent-doc start /home/brian/work/btakita/agent-loop/tasks/agent-doc/agent-doc-bugs2.md` can hang after a prior managed pane is closed by `agent-doc`.

Observed evidence from the current workspace:

- `agent-doc session status ...agent-doc-bugs2.md` and `agent-doc controller status --project-root /home/brian/work/btakita/agent-loop` both hang until an external `timeout` kills them.
- The project controller process has accepted a controller socket connection and is blocked on it.
- The peer is an old `agent-doc sync --no-autostart ...agent-doc-bugs2.md ...` process that still holds `.agent-doc/sync.lock` and an open controller socket.
- The actor projection still points `agent-doc-bugs2.md` at pane `%261`, but current `tmux list-panes -a` no longer contains `%261`.

The likely root cause is that the controller server handles one accepted client connection to completion before accepting the next client. A client that connects and then stalls can monopolize the controller. All later RPC callers block behind that accepted socket, so the user sees a hang rather than the intended stale-pane / closed-pane recovery guidance.

## Fix Strategy

1. Make controller client reads bounded.

   Add a read deadline or equivalent timeout around controller request/response reads. The client should fail closed with an actionable error when no response arrives within a small bounded interval.

2. Make controller serving resilient to idle clients.

   Replace the single blocking `accept -> read all lines -> next accept` loop with either:

   - one thread per accepted client, or
   - a per-connection read timeout that closes idle clients and returns to `accept`.

   The key invariant is that one stalled client must not block unrelated controller RPCs.

3. Remove idle returned connection hazards.

   Audit `connect_or_launch` callers so it does not return an open idle stream that a caller can hold while doing other work. Prefer an `ensure_controller_running(project_root, launch_mode) -> Result<()>` helper for readiness checks, and reserve request streams for calls that immediately write a framed request.

4. Teach `sync --no-autostart` to release controller access before slow tmux/layout work.

   The sync path should not hold any controller client socket across tmux layout reconciliation, closeout recovery, prompt proof waits, or lock waits. If a controller query is needed, complete it, close the stream, then continue.

5. Surface stale closed-pane diagnostics instead of hanging.

   When `start` sees an actor or registry owner pane that no longer exists, it should continue into the existing missing-pane recovery/fail-closed path. The message should name the stale pane, the active actor generation, and the repair/status commands.

## Tests

- Controller unit/integration test: a client connects to `.agent-doc/controller.sock` and sends no newline; a second client can still complete `status`.
- Controller client test: a request with no response returns a timeout error instead of blocking indefinitely.
- `run_status --ensure` regression: ensure-mode must not self-deadlock by holding an idle stream while issuing the status request.
- Sync regression: `sync --no-autostart` with a dead actor pane cannot monopolize the controller socket.
- Start regression: with actor projection pointing at a dead pane, `start` fails closed or repairs according to existing policy within a bounded timeout.

## Validation

Run focused tests first:

```bash
cargo test -p agent-doc project_controller -- --nocapture
cargo test -p agent-doc sync -- --nocapture
cargo test -p agent-doc start -- --nocapture
```

Then run the full required suite:

```bash
make check
```

