# Plan: Startup-Miss Diagnostic Dispatch

**Pending:** `#smdq` in `tasks/agent-doc/agent-doc-bugs2.md`
**Status:** Draft

## Problem

`agent-doc` already records startup-miss markers when a routed trigger or fresh start never opens a new document cycle, but the in-pane diagnostic path is still broken for live Codex sessions.

Concrete repro from April 29, 2026:

- `tasks/agent-doc/agent-doc-bugs2.md` routed `do #ppct. spec-test-build-install-commit-push` into session `b26b9957-8de6-4eae-bb98-5d7fe4d6b781`
- `ops.log` recorded `route_cycle_start_missing file=tasks/agent-doc/agent-doc-bugs2.md pane=%399 harness=codex`
- the pane stayed alive, but the user-facing surface looked broken because the pane showed a drafted line:
  - `echo '[agent-doc] startup-miss: routed trigger accepted but no document cycle started ...'`
  - with Codex UI text `tab to queue message`
- so the diagnostic was staged as unsent harness input instead of appearing as visible output

This matches the current implementation in `route.rs`: `emit_startup_miss_diagnostic()` uses `tmux.send_keys_raw(...)` with the literal `echo '...'` text, but does not prove that the text is submitted or rendered. In a Codex pane, that can leave the message sitting in the prompt box and make the user read the pane as "tmux crashed" or hung.

## Goal

Make startup-miss diagnostics binary-owned, visible, and deterministic so a missed routed-cycle start never degrades into an unsent draft inside the harness input area.

## Desired Behavior

- when `route_cycle_start_missing` or the equivalent fresh-start timeout happens, the user immediately sees a startup-miss diagnostic without needing to inspect logs
- the diagnostic is rendered as output or another deterministic UI surface, not left as pending input in a Codex/Claude prompt box
- the pane remains clearly recoverable:
  - exact reason
  - affected document
  - retry command
- reruns can distinguish:
  - real dead pane
  - live pane with startup-miss marker
  - stale startup-miss marker that can be cleared

## Implementation Steps

- [ ] Audit `emit_startup_miss_diagnostic()` and the tmux helper semantics for Codex, Claude, and plain-shell panes
- [ ] Decide the binary-owned delivery mechanism for startup-miss diagnostics:
  - submit a newline-terminated shell echo only when the pane is actually at a shell
  - or write through a safer pane-visible channel that does not depend on harness input state
  - or split operator messaging between pane output and tmux/status diagnostics
- [ ] Prove that the chosen path cannot leave unsent recovery text in the harness composer/input buffer
- [ ] Keep the recovery hint actionable:
  - `agent-doc start <file>`
  - any session-check or startup-miss provenance that helps explain the failure
- [ ] Reconcile the implementation with the current spec text that says route "echoes a diagnostic into the pane"
- [ ] Add regressions for:
  - routed Codex startup miss -> diagnostic is visible and not left as pending input
  - fresh-start timeout -> same visible diagnostic contract
  - stale startup-miss marker cleared after successful later ownership proof
  - live pane rerun after startup miss does not look like a literal pane crash

## Files Likely To Change

- `src/agent-doc/src/route.rs`
- `src/agent-doc/src/tmux.rs`
- `src/agent-doc/src/session_check.rs`
- `src/agent-doc/specs/07-commands.md`
- `src/agent-doc/VERSIONS.md`

## Exit Criteria

- the `%399` / `#ppct` reproduction no longer leaves a drafted `echo ...` line in the Codex prompt area
- startup-miss diagnostics become visible without log inspection
- users no longer experience this failure as an unexplained tmux-pane crash when the pane is actually still alive
