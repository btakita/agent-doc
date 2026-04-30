# Plan: Snapshot desync after crash recovery (#snpsync)

## Problem

When a tmux pane crashes at `preflight_started`, `agent-doc recover` repairs and commits the stale cycle. But the snapshot file retains pre-crash normalization state (e.g., `❯ ` prefixes on user prompt lines) that the document no longer has. `session-check` then sees a diff between snapshot and document and classifies the already-responded-to prompt as "unresolved prompt-bearing user changes" — even though the response is committed directly below it.

This forces the skill to run a second full `finalize` cycle just to sync the snapshot, wasting a turn.

## Repro

From `src/session-share/tasks/claudescore.md` on Apr 29, 2026:

1. Tmux pane crashed at `preflight_started` (cycle `cycle-1777506550775`)
2. `agent-doc recover` repaired the stale cycle → committed
3. Snapshot still had `❯ do #wdiv. spec-test-news-commit-push` but document had `do #wdiv. spec-test-news-commit-push` (no prefix)
4. `session-check` exited 1: "unresolved prompt-bearing user changes" on the already-responded prompt
5. Required a second `finalize` cycle with empty normalization content to sync the snapshot

## Root Cause

`recover` repairs the cycle state machine and commits, but does not re-save the snapshot from the committed document content. The snapshot remains a stale copy from the last successful cycle, which may have different normalization (prefix presence, whitespace, boundary markers) than the current committed document.

## Proposed Fix

### Phase 1: Re-snapshot after recover commit

In `recover`, after successfully repairing a stale `preflight_started` cycle and committing:

1. Read the committed document content from HEAD
2. Re-save the snapshot from that content
3. This ensures snapshot matches the committed state, preventing phantom diffs

### Phase 2: session-check normalization tolerance

As a defense-in-depth measure, teach `session-check`'s "unresolved prompt-bearing changes" detector to ignore diffs that are purely normalization artifacts:

- `❯ ` prefix presence/absence on prompt lines
- `(HEAD)` boundary marker additions/removals on response headers
- Whitespace-only differences

If the only diffs are normalization artifacts AND a response heading exists below the prompt line, session-check should pass.

## Related

- `#dupfx` — duplicate response when session-check fails on prefix and recovery replays
- `#tmuxcrash` — broader tmux crash stability
