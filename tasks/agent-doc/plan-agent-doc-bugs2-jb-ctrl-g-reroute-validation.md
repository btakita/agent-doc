# Plan — validate the JB `Run Agent Doc` reroute after the `C-g` busy-pane recovery probe

## Problem

The latest JetBrains `Run Agent Doc` failure for `tasks/agent-doc/agent-doc-bugs2.md` still failed after the existing same-document busy-pane ladder:

- route recovered the authoritative live pane for the same document
- scoped `agent-doc fix` made no reusable progress
- the healthy supervisor restarted fresh once
- the final bounded interrupt still left the pane blocked and the reroute failed closed

The implementation now adds a short `C-g` probe before the older `Escape` + `C-c` closeout so latent shell-history search states can recover into a real idle Codex prompt, but the actual JB-triggered repro still needs a live replay.

## Goal

Prove that the JetBrains reroute now does one of these deterministically:

1. recovers the existing authoritative pane and starts a new document cycle
2. fails once with a stage-specific blocker that survives the `C-g` probe and the fallback interrupt

## Validation steps

- [ ] Re-run the original JB `Run Agent Doc` repro against `tasks/agent-doc/agent-doc-bugs2.md`.
- [ ] Confirm the busy-pane ladder reaches the new `C-g` readiness probe before the legacy `Escape` + `C-c` fallback.
- [ ] Verify the reroute still sends only the bare `agent-doc <absolute-file>` reopen after recovery.
- [ ] Check whether the reroute starts a fresh document cycle (`preflight_started` and later closeout) for `agent-doc-bugs2.md`.
- [ ] If it still fails closed, capture the surviving blocker reason and whether it appeared during the `C-g` probe or only after the fallback interrupt.

## Exit criteria

- The JB reroute either succeeds end-to-end or narrows to one explicit remaining blocker with preserved recovery-stage provenance.
- No regression weakens the bare-reopen or fail-closed safety contract for routed Codex dispatch.
