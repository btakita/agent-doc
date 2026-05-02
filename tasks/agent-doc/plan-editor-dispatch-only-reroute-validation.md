# Plan — validate editor dispatch-only `Run Agent Doc` reroutes

**Pending:** `#edispatch` in `tasks/agent-doc/agent-doc-bugs2.md`
**Status:** Draft

## Problem

The editor-triggered Run path now uses `agent-doc route --dispatch-only` instead of the heavier managed reroute flow:

- JetBrains saves and dispatches immediately with no editor-side typing debounce
- VS Code no longer blocks Run behind a local "Command already in progress" gate
- dispatch-only route resolves the owning pane and sends the bare `agent-doc <FILE>` reopen without route-owned startup-miss gating, `/clear` relaunch policy, busy-pane recovery, or cycle-ack waiting

That should stop the editors from fail-closing on stale route state, but it still needs live validation in both editor integrations against real tmux-bound Codex sessions.

## Goal

Confirm that JetBrains and VS Code both reopen the correct pane reliably after repeated manual Run actions, including after cancel + `/clear`, without regressing pane targeting.

## Replay Steps

- Start a real Codex-owned session for `tasks/agent-doc/agent-doc-bugs2.md`
- In JetBrains, trigger `Run Agent Doc` several times, including once immediately after cancel + `/clear`
- In VS Code, repeat the same manual Run sequence on the same document
- Confirm each editor invocation reaches the correct tmux pane and sends the bare reopen promptly
- Confirm the editor UI no longer suppresses the reroute with a local in-flight gate or typing debounce delay

## Evidence To Capture

- JetBrains notifications / logs for repeated Run invocations
- VS Code status / error output for repeated Run invocations
- tmux pane scrollback proving the bare `agent-doc <FILE>` reopen was sent
- any remaining failure shape where the editor did invoke dispatch-only route but the reopen still did not execute as intended

## Exit Criteria

- JetBrains Run sends immediately without plugin-local debounce or in-flight blocking
- VS Code Run does not suppress a second manual reroute while a previous subprocess is still winding down
- both editors still target the correct tmux pane for the document session
