# Plan: Fix finalize --pending-done cascade failure

## Problem

When a `do #id` queue directive triggers finalize with `--pending-done <id>`, the closeout requires up to 4 attempts due to conflicting guards:

1. **First finalize** fails: `pending_capture_guard` fires because "active prompt requested backlog capture but no backlog mutations were recorded" — even though the intent is to mark the item done, not add new items.
2. **Second finalize** with `--pending-done <id>` fails: `pending_done_guard` fires because `repair` already reaped the completed item, AND `pending_capture_guard` still fires.
3. **repair** needed to reap stale completed item before retry.
4. **Third finalize** with `--pending-done <id>` fails: "id not found in backlog/icebox" because repair already reaped it.
5. **Fourth finalize** with `<!-- no-pending-capture -->` finally succeeds.

## Root Cause

Two guards conflict during `do #id` closeout:

- `pending_done_guard`: expects `--pending-done` to be used, but the item may already be reaped by `repair`.
- `pending_capture_guard`: expects new backlog items to be added when the prompt is "actionable," but a `do #id` prompt where the work is done has no follow-up items.

The guards don't coordinate. A `do #id` with `--pending-done` should satisfy BOTH guards in one shot.

## Fix

### Phase 1: Atomic `do #id` closeout

- When `finalize` receives `--pending-done <id>`, treat this as satisfying the `pending_capture_guard` too — the prompt's actionability is resolved by the done marker.
- If the item was already reaped (by a prior repair cycle), `--pending-done <id>` should be a no-op warning, not a fatal error.

### Phase 2: Guard coordination

- Add a `prompt_intent` field to the finalize context that classifies the cycle as `do_id`, `new_work`, `question`, etc.
- For `do_id` intent: `--pending-done` alone satisfies both guards.
- For `new_work` intent: `pending_capture_guard` still requires `--pending-add` or explicit `<!-- no-pending-capture -->`.

### Phase 3: Regression test

- Add a SimWorld test: `do #id` → finalize with `--pending-done <id>` succeeds in one shot.
- Add a SimWorld test: `do #id` where item was already reaped → finalize with `--pending-done <id>` warns but succeeds.
- Add a SimWorld test: `do #id` with follow-up work → finalize with `--pending-done <id> --pending-add <new>` succeeds.
