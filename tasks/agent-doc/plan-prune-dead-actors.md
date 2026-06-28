# Plan: auto-prune dead actor records (#actorprune)

## Problem
`agent-doc admin list` shows 251 actors in `src/agent-doc`, essentially all `(dead) ... closed pid=? session=session-clear` (operator reported ~60; it has since grown). Dead/closed actor records accumulate in the actor store forever — nothing prunes them.

## Root cause
`gc.rs` only calls `close_stale_starting_actors` (`project_controller.rs:1716`), which TRANSITIONS `Starting` actors to closed after a staleness window. It never **removes** records already in `Closed`/dead state. The actor store (`load_actor_store(project_root)`) keeps every closed record indefinitely, so `admin list` grows unboundedly.

## Fix
1. Add `prune_dead_actors(project_root, dead_after: Duration, dry_run)` to `project_controller.rs`: remove store records that are `Closed` (or whose `pid` is provably dead) AND whose `last_transition.timestamp` is older than `dead_after`, with NO fresh/alive supervisor lease for that `document_id`/generation (reuse the `supervisor_lease_is_fresh_or_alive` guard so a live actor is never pruned). Never prune the most-recent record for a still-tracked document (keep one tombstone per active doc if needed).
2. Wire it into `gc.rs` alongside `close_stale_starting_actors` (default-on, dry-run honored), and expose via the existing `agent-doc gc` / a one-shot `admin prune-actors` path so the operator can clean the current 251 immediately.
3. Log each prune (`actor_pruned document_id=… generation=… state=… age_secs=… reason=dead_closed_record`) — never silent.

## Tests
- `prune_dead_actors_removes_old_closed_records`
- `prune_dead_actors_keeps_live_lease` (fresh lease ⇒ kept even if closed-looking)
- `prune_dead_actors_keeps_recent_closed` (within window ⇒ kept)
- `gc_prunes_dead_actor_records` end-to-end (dry-run preserves)

## Ship
bump → VERSIONS → make check → commit → install-full (in a focused cycle, NOT mid-recovery) → push + tag. Skip registry publish per the standing pause.
