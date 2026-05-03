# Plan — validate optimistic JetBrains `Run Agent Doc` reroutes after cancel + `/clear`

**Pending:** `#jboptim` in `tasks/agent-doc/agent-doc-bugs2.md`
**Status:** Draft

## Problem

The code path now does the optimistic thing the latest repro asked for:

- JetBrains no longer blocks a second `Run Agent Doc` click with a local "Route already in progress" gate
- `agent-doc route` still checks that the tmux pane is bound to the requested file
- once the live Codex pane accepts the bare reopen, route records startup-miss diagnostics but does not fail closed just because a later cycle-ack or fresh-restart readiness proof never materializes
- the alive-pane busy-session ladder now keeps that same optimistic contract after scoped fix, fresh restart, and bounded interrupt recovery

That resolves the false-negative "did not run agent-doc" behavior in source, but it still needs a real JetBrains replay against the original cancel + `/clear` workflow.

## Goal

Confirm that the live JetBrains `Run Agent Doc` workflow now reliably reopens the correct pane after cancel + `/clear` without surfacing the old local gate or route-level false negative.

## Replay Steps

- Start a real Codex-owned session for `tasks/agent-doc/agent-doc-bugs2.md`
- Cancel the active turn
- Run `/clear`
- Trigger JetBrains `Run Agent Doc` again one or more times in quick succession
- Confirm the routed reopen lands in the same authoritative pane (or its fresh-restart replacement) without the old plugin-side block
- Confirm any remaining startup-miss text is diagnostic-only and does not suppress the routed reopen itself

## Evidence To Capture

- JetBrains action log / notification behavior after repeated `Run Agent Doc` invocations
- `ops.log` entries showing optimistic live-pane reroute handling
- whether a startup-miss marker is recorded and then cleared on the next real cycle start
- whether any remaining failure shape still leaves `agent-doc` unrun when the correct pane was known

## Exit Criteria

- Repeated JetBrains `Run Agent Doc` after cancel + `/clear` no longer gets blocked locally
- The correct tmux pane receives the reopen even when route cannot prove a later cycle-ack immediately
- Any residual failure is a real execution problem, not a false-negative "refusing to run" gate
