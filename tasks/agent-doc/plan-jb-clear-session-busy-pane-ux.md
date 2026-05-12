# Plan: JetBrains Clear Session Context Busy Pane UX

## Problem

The JetBrains `Clear Session Context` command currently surfaces the raw fail-closed
CLI error when the authoritative pane is alive and busy:

```text
session_clear refused ... because pane %1 is alive-busy
```

That guard is correct: clearing a pane with a live `agent-doc` turn could discard
in-flight work or lose the closeout boundary. The UX problem is that the editor command
looks like an unexpected failure even though agent-doc is protecting a running session.

## Evidence

- `agent-doc session status tasks/agent-doc/agent-doc-bugs2.md` reports
  `live_pane: state=alive-busy pane=%1 source=authoritative_actor`.
- The pane tail is an active Codex status line, not an idle harness prompt.
- The supervisor and controller both still consider the actor healthy/ready, so the
  refusal comes specifically from direct live-pane evidence.

## Plan

1. Keep `session clear <FILE>` fail-closed when direct tmux evidence says the pane is
   `alive-busy`.
2. Add a JetBrains-facing command result for protected busy panes so the IDE can show
   a non-crash notification such as "Session is still running" with the pane id and
   latest status tail.
3. Add an explicit interrupt flow, separate from safe clear, for users who intentionally
   want to discard the running turn. It should confirm intent, send the harness-specific
   interrupt, wait for an idle prompt or closed pane, then retry clear.
4. Add a retry/wait path for the common case where the pane is about to return to an
   idle prompt.
5. Add regressions for safe clear refusing `alive-busy`, IDE command output mapping,
   and explicit interrupt-and-clear success.

## Verification

- Unit coverage for the typed busy-pane clear result.
- JetBrains integration test or fixture that maps the typed result to a user-facing
  notification instead of a generic command failure.
- tmux-backed smoke test for explicit interrupt-and-clear on a controlled busy pane.
