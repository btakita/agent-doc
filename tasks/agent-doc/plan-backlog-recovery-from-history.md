# Plan — recover dropped backlog items from recent history

**Pending:** `#backlogreplay`
**Status:** Draft

## Problem

Open backlog items can disappear from the live `agent:backlog` entirely, even when recent document history proves they were still unfinished. This is adjacent to `#backlogshadow`, but narrower and more severe:

- `#backlogshadow` covers open items drifting into parked/commented sections
- this bug covers open items being absent from both the live backlog and the visible parked sections, with recovery only discoverable by inspecting recent commits or snapshots

The `claudescore-3.md` repro is concrete:

- commit `541da7c` left the live backlog with only `#outcome`
- the unfinished digest items `#sdig2`, `#sdig2fx`, and `#sredact` were missing from the active backlog surface
- commit `4f25d47` had to restore those still-open items from recent history

The current document is already back in the correct state, but the recovery was manual and history-driven rather than binary-owned.

## Goal

When a write/finalize/repair cycle would leave a session document with fewer open backlog items than the recent committed history supports, `agent-doc` should surface that loss explicitly and either:

- fail closed before accepting the new state, or
- offer a deterministic recovery path using recent snapshots / git history.

## Desired Behavior

- abrupt disappearance of open backlog ids from the live backlog is detected before closeout reports success
- the detector can compare:
  - current live `agent:backlog`
  - current parked/commented backlog-shaped items
  - prior committed/snapshotted backlog ids from the same document
- if an item was previously open and no completion evidence exists, dropping it should not be silently accepted
- recovery guidance should identify the likely missing items and their most recent surviving text

## Implementation Steps

- [ ] Define the recovery window and evidence sources:
  - last committed document state
  - latest saved snapshot
  - optionally a bounded recent git-history window for the same file
- [ ] Define what counts as a backlog-loss event:
  - open ids removed from live backlog
  - no matching completion evidence in the exchange
  - no surviving copy in shadow/commented sections
- [ ] Audit `preflight`, `finalize`, `repair`, and `session-check` for the best insertion point to compare prior and current backlog sets
- [ ] Add a detector that emits a specific class such as `backlog_items_missing_from_recent_history`
- [ ] Decide the contract:
  - fail closed when open items disappear with no completion proof
  - warning/recovery guidance when the binary can name the missing items but not restore them safely
  - optional auto-restore only when the prior text is unambiguous and the write still belongs to the active cycle
- [ ] Ensure this composes with `#backlogshadow`:
  - shadow copy present -> classify as shadow loss
  - no shadow copy, only recent-history proof -> classify as history-backed loss
- [ ] Add regressions for:
  - normal reap after explicit completion
  - open items removed but still present in comments
  - open items removed from both live and comment surfaces, recoverable from prior commit
  - item text changed legitimately while id stayed live
  - current repro shape from `541da7c -> 4f25d47`

## Files Likely To Change

- `src/agent-doc/src/preflight.rs`
- `src/agent-doc/src/finalize.rs`
- `src/agent-doc/src/repair.rs`
- `src/agent-doc/src/session_check.rs`
- `src/agent-doc/src/diff.rs`
- `src/agent-doc/src/git.rs`
- `src/agent-doc/specs/07-commands.md`

## Exit Criteria

- open backlog items cannot silently disappear from the active backlog when recent history proves they were still unfinished
- `agent-doc` can distinguish shadow backlog loss from full recent-history backlog loss
- the `claudescore-3.md` `541da7c -> 4f25d47` recovery path is either prevented up front or surfaced with precise restoration guidance
