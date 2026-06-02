# Plan: lazily-rs queue head deferral proof

## Problem

In `tasks/software/lazily-rs.md`, a user report combined an active
`#agent-doc-bug` capture with a runnable `agent:queue auto` prompt:

```text
preset #spec-test-build-install-commit-push
- Can `CellHandle::set(&ctx, value)` also apply to the multi-threaded Context?
```

`agent-doc plan` classified the cycle as `execution_scope=plan_backlog_only`.
That is correct for the bug-capture portion, but the planning output did not
make the actionable queue-head deferral explicit enough for closeout. The user
expects the queue head to be answered or left visibly queued for the next turn,
not hidden behind the bug-planning side effect.

## Desired Behavior

When a diff contains both:

- a planning-only `#agent-doc-bug` transfer, and
- an active runnable queue head with a prompt preset,

the planning contract must represent both. If the cycle is limited to
`plan_backlog_only`, it must emit a concrete deferral proof naming the exact
queue head that remains unresolved.

## Implementation Plan

1. Add a planner fixture for a template-mode document with a mixed
   `#agent-doc-bug` report and an active free-text preset queue head.
2. Split the planning record into a bug-capture action and a queue-head action
   instead of collapsing both into one `plan_backlog_only` decision.
3. If `plan_backlog_only` still wins, add a machine-readable field such as
   `deferred_queue_heads` containing the queue prompt text and preset expansion.
4. Teach closeout/session-check to accept that deferral proof only when the
   response body states the deferral and the queue head remains active.

## Tests

- Planner regression: mixed bug capture plus runnable queue prompt emits
  pending-add-to for the bug and explicit queue-head deferral proof.
- Closeout regression: the deferred queue head remains in `agent:queue auto`
  after the bug-capture response commits.
- Negative regression: a response that neither answers nor explicitly defers the
  head fails the pending/queue guard before commit.

## Exit Criteria

- The lazily-rs repro cannot look completed while the runnable queue head is
  silently deferred.
- Later `agent-doc /path/to/lazily-rs.md` cycles still see the unresolved
  multi-threaded `CellHandle::set` prompt.
