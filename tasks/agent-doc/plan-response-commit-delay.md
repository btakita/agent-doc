# Plan: response patchback commit delay

## Problem

A session response can be written or partially repaired, but the final committed
state is delayed by fallback/recovery churn. The user sees the response before
the commit boundary is actually closed, then has to wait through failed
`write --commit` / `finalize` attempts, manual component patching, repair, and
parent pointer closeout before the response is durable.

Current evidence:

- Document: `src/session-share/tasks/root.md`
- User report: "The response had a long delay before being committed."
- Shape observed: `agent-doc write --commit` and `agent-doc finalize` rejected
  fallback patchback because the cycle was already marked committed while the
  prompt-bearing exchange drift was still uncommitted.
- Recovery required manual `agent-doc patch`, `agent-doc commit`, stale
  `preflight_started` repair, and a parent submodule pointer commit before
  `agent-doc session-check` passed.

This is related to `#rspcmt*`, `#partialpatchbackadopt`, and
`#rootlatepatch`, but the user-facing bug is latency: closeout does not quickly
reach either "committed" or "failed with one exact recovery command."

## Desired Behavior

1. Once a response patchback is visible, the commit boundary should close in one
   bounded path, ideally inside the same command that wrote the response.
2. If the response cannot be committed immediately, agent-doc should fail fast
   with the exact missing layer and a single idempotent recovery command.
3. `write --commit` / `finalize` should adopt an already-visible response when
   it is clearly agent-doc-owned instead of rejecting it as late fallback drift.
4. Parent submodule pointer commits should be part of the same reported
   closeout phase, with elapsed timing surfaced separately from response
   generation time.

## Investigation Steps

- [ ] Reconstruct the `src/session-share/tasks/root.md` delayed closeout path:
      failed `write --commit`, failed `finalize`, manual component patch,
      `agent-doc commit`, stale preflight repair, and parent pointer commit.
- [ ] Identify which state transition caused the response to be visible before
      the cycle could be committed cleanly.
- [ ] Compare with existing plans:
      `plan-response-patchback-uncommitted.md`,
      `plan-late-fallback-patch-redirties-committed-doc.md`, and
      `plan-empty-response-repair-write.md`.
- [ ] Add timing instrumentation for write, IPC/plugin delivery, git commit,
      session-check, repair, and parent pointer closeout.

## Implementation Steps

- [ ] Add an adoption path for visible agent-doc-owned response blocks when the
      cycle was marked committed but prompt/response drift remains.
- [ ] Make `write --commit` / `finalize` avoid starting new preflight cycles
      while repairing a previous closeout.
- [ ] Collapse target repo commit and parent pointer commit into one closeout
      transaction from the user's perspective.
- [ ] Surface a phase-by-phase closeout timer when response-to-commit latency
      exceeds a small threshold.
- [ ] Fail closed with one recovery command if any closeout layer remains dirty
      after the bounded path.

## Regression Coverage

- [ ] Simulate a visible response patchback plus already-committed cycle state;
      recovery should adopt and commit it without duplicate patchback.
- [ ] Simulate prompt-bearing exchange drift after a committed snapshot;
      closeout should distinguish unresolved user prompt from agent-owned
      response repair.
- [ ] Simulate submodule target commit success with parent pointer still dirty;
      closeout should finish the parent commit or fail with exact guidance.
- [ ] Assert response-to-commit latency events are emitted when closeout crosses
      the configured budget.

## Exit Criteria

- A response patchback cannot sit visible for an extended period before commit
  without a live progress message naming the active closeout phase.
- Normal closeout either commits all required layers quickly or fails with one
  precise, idempotent recovery command.
