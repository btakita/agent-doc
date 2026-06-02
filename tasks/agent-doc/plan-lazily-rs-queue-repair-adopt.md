# Plan: lazily-rs queue response repair adoption

## Problem

The same `tasks/software/lazily-rs.md` repro asked:

```text
If you did process it, write the response back into this document.
```

That is a missed-patchback recovery request. If an earlier agent completed the
multi-threaded `CellHandle::set` work or answered in the console but did not
cross `finalize` / `write --commit`, the response does not exist in the session
document. The recovery path must adopt a supplied response through the
binary-owned commit boundary without rerunning completed side effects.

## Desired Behavior

- `agent-doc write --commit <file>` with a supplied response body attaches the
  response to the correct exchange boundary.
- A matching free-text queue head is either consumed by the binary-owned queue
  path or left active with explicit proof if the response is only a bug-capture
  deferral.
- Empty repair attempts fail before child-agent dispatch and name the missing
  response-body requirement.

## Implementation Plan

1. Add a fixture where the session document contains a queued free-text prompt,
   completed side-effect proof in a prior console transcript, and no committed
   assistant response.
2. Extend repair/write adoption so a supplied template response can target the
   newest unresolved exchange prompt even when the queue head is also active.
3. Preserve side-effect boundaries: the repair must not rerun repo work or
   commit unrelated dirty files.
4. Refine diagnostics for empty or already-committed repairs so operators get a
   single actionable command instead of an empty child-agent handoff.

## Tests

- `write --commit` regression: supplied repair response lands in the exchange,
  commits, and session-check passes.
- Queue regression: if the repair response answers the active free-text queue
  head, queue state advances atomically with the session-document commit.
- Negative regression: empty stdin does not dispatch to Codex/Claude and does
  not mutate queue state.
- Submodule regression: completed lazily-rs side-effect commits are not
  recreated or staged by the repair.

## Exit Criteria

- Missed-response recovery is one binary-owned operation.
- The response body is present in `HEAD` before success is reported.
- Queue state after repair matches the response outcome.
