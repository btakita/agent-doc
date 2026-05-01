# Plan — validate the `claudescore-3.md` busy-reroute recovery after the bounded interrupt fix

## Problem

The May 1, 2026 `src/session-share/tasks/claudescore-3.md` reroute still failed closed after the existing busy-pane recovery ladder:

- route detected prompt-bearing drift for the same document
- it ran the scoped `agent-doc fix`
- it restarted the healthy live Codex supervisor fresh once
- the reroute still ended in `refusing to inject a routed trigger into a busy session`

The new route change adds one bounded same-document interrupt recovery before the final fail-closed error, but that live `claudescore-3.md` repro still needs to be replayed against a real tmux/Codex pane.

## Goal

Prove that the new interrupt-recovery path turns this repro into either:

1. a successful same-document reroute that starts a new `agent-doc` cycle, or
2. a clearer final fail-closed state that preserves the bare reopen contract and leaves precise diagnostics about what blocked consumption

## Validation steps

- [ ] Re-run the original JetBrains or `agent-doc route tasks/claudescore-3.md` repro against the live pane family that previously reported `reverse-i-search` / busy-session refusal.
- [ ] Confirm route sends the interrupt recovery only after the existing scoped-fix + fresh-restart ladder is exhausted.
- [ ] Verify the final payload is still the bare `agent-doc tasks/claudescore-3.md` reopen and not a multiline draft.
- [ ] Check whether the reroute now reaches a new document cycle (`preflight_started` / later committed closeout) for `claudescore-3.md`.
- [ ] If it still fails closed, capture the final pane state and the exact blocker reason after the interrupt attempt so the remaining gap is diagnosable.

## Exit criteria

- The live `claudescore-3.md` busy reroute is either fixed end-to-end or narrowed to a new explicit blocker with preserved route/ack provenance.
- No regression drops the existing safety invariant against injecting multiline or drafted-composer payloads into Codex.
