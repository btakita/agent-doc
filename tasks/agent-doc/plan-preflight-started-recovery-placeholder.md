# Plan: Recovery from stuck `preflight_started` cycle writes placeholder instead of handling prompt

## Bug

When a harness pane is reaped after `preflight` started but before the response
cycle completed, the next `agent-doc preflight` (or `recover`) errors out:

```
empty preflight_started cycle has no response capture … Restart the harness pane
and rerun `agent-doc <FILE>`
```

The only current recovery is `agent-doc write --commit` with a manual placeholder
response. That placeholder doesn't execute the user's original prompt — it just
unblocks the cycle so a fresh preflight can run. The user's prompt goes unanswered
and must be manually retried.

## Root cause

`preflight` detects the stuck `preflight_started` cycle and sees an unresolved
`prompt_target` in the document but no response capture. It refuses to start a new
cycle because the previous one never reached a committed state, and there's no
response body to replay.

## Desired behavior

A new harness instance picking up a document with a stuck `preflight_started`
cycle should be able to:

1. Abandon the stuck cycle (transition to `abandoned` or `recovered` state)
2. Start a fresh cycle that inherits the unresolved `prompt_target` from the
   abandoned cycle
3. Process the prompt normally — the user's edit should not require re-entry

## Plan

### Phase 1: Cycle state transition

- Add an `abandoned` terminal state for cycles that were started but never
  produced a response
- `preflight` detects `preflight_started` with no response capture and
  transitions it to `abandoned` (with reason: `pane_reaped_before_response`)
- Log the abandoned cycle for diagnostics

### Phase 2: Prompt carryover

- When abandoning a cycle, preserve the unresolved `prompt_target` list
- The new cycle's preflight diff should still detect the user's edit as a
  prompt-bearing change (it already does — the edit is in the live document)
- Verify that the baseline used for diffing is from before the stuck cycle's
  preflight (the last committed state), not from the stuck cycle's snapshot

### Phase 3: Recover command support

- `agent-doc recover <FILE>` should also handle `preflight_started` stuck
  cycles by abandoning them (currently it errors identically to preflight)
- Add `--abandon-stuck` flag for explicit abandon without writing a placeholder

### Tests

- Unit test: cycle in `preflight_started` state → new preflight abandons it and
  starts a fresh cycle
- Integration test: simulate pane reap (kill cycle mid-preflight) → new harness
  picks up the unresolved prompt
- Regression test: ensure a legitimately running `preflight_started` cycle (pane
  still alive) is NOT abandoned by a concurrent preflight
