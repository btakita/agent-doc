# Plan: --pending-add inserts multiple items in reverse order

## Problem

When multiple `--pending-add` flags are passed in a single `agent-doc write` or `agent-doc finalize` command, items are inserted in reverse order relative to the user's intent.

**Example:** The skill passes:
```
--pending-add "id=vt-avatar-tracking ..." --pending-add "id=vt-agent-session ..."
```

The user intended `#vt-avatar-tracking` to be first (highest priority), `#vt-agent-session` second. But in the resulting backlog, `#vt-agent-session` appears above `#vt-avatar-tracking`.

## Root Cause

Each `--pending-add` prepends the new item to the top of `agent:backlog`. When processing multiple adds sequentially, the second add pushes above the first — stack-like (LIFO) behavior instead of queue-like (FIFO).

## Fix

In the pending mutation handler that processes `--pending-add` flags:

1. **Option A (preferred):** Collect all `--pending-add` items into a batch, then insert them all at once at the top of the backlog in the order they were specified. This preserves the caller's intended priority order.

2. **Option B:** Reverse the order of `--pending-add` items before processing, so the last item gets prepended first and the first item ends up on top.

## Files to Change

- The CLI argument parser that collects `--pending-add` values (likely in `src/agent-doc/src/` — the write/finalize command handler)
- The pending mutation function that inserts items into the backlog component

## Test Cases

1. Single `--pending-add` — item appears at top of backlog (no change)
2. Two `--pending-add` flags — first flag's item appears above second flag's item
3. Three+ `--pending-add` flags — all items appear in specified order at top of backlog
4. Mixed `--pending-add` + `--pending-done` — add order preserved, done items processed independently
