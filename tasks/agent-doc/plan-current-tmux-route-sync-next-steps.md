# Plan: current tmux route and sync next steps

**Pending:** `#next-steps`
**Status:** Draft
**Evidence window:** May 12 ops logs for `agent-doc-bugs2.md`, `ron-berkes.md`, and related active tmux sessions

## Problem

Recent logs show the tmux/controller path is mostly healthy on the happy path, but three
failure classes still leak through during startup, route, and editor sync:

- `actor_binding` can return `{"ok":true,"data":null}` and the caller logs
  `controller_response_missing_data` repeatedly instead of treating it as a typed
  absent-binding result.
- Freshly started actors still miss the ready window. `ron-berkes.md` generation 8
  logged four `route_authoritative_actor_starting_not_ready` failures at about 9s,
  then `route_dispatch_only_actor_state_direct_pane_submit` while the actor was
  `busy`, followed by `route_dispatch_only_starting_pane_not_ready`.
- Sync latency is improved in the latest tail, but recent same-day evidence still shows
  `sync_lock_wait` spikes around 3s, `controller_actor_lookup` over the 250ms budget,
  and occasional prune over-budget events.

## Goal

Make tmux routing boring again: route only after the controller has a coherent actor
binding and a dispatch-ready prompt, editor sync should not serialize behind redundant
work, and diagnostics should point to one exact failed phase.

## Phase 1: make controller actor binding typed and fail-closed

Files: `src/agent-doc/src/project_controller.rs`, `src/agent-doc/src/route.rs`,
`src/agent-doc/src/sync.rs`

1. Change `actor_binding` responses so absent bindings are explicit (`not_found` or
   `data:null` with an explicit status), not `ok:true` without data.
2. Make route and sync branch on the typed status instead of logging
   `controller_response_missing_data` and continuing through ambiguous fallback paths.
3. Add regressions for:
   - no actor exists for a document
   - controller returns malformed/missing data
   - sync safe-passive focus with no binding remains a no-op with a precise diagnostic

Acceptance criteria:

- no `controller_response_missing_data command=actor_binding raw={"data":null,"ok":true}`
  entries in a normal editor navigation burst
- route and sync messages distinguish `no binding`, `stale binding`, and `controller
  protocol error`

## Phase 2: replace startup sleep budgets with a ready-prompt barrier

Files: `src/agent-doc/src/route.rs`, `src/agent-doc/src/start.rs`

1. Extend the existing `plan-route-starting-actor-race-after-restart.md` work: stop
   relying on a fixed 8s Codex route wait as the deciding boundary.
2. Route should subscribe/poll for a controller lifecycle transition to `ready` plus
   dispatch-ready prompt proof before allowing dispatch-only reopen.
3. If the actor transitions through `busy` during restart bootstrap, treat that as
   `booting`, not as permission for direct pane submit.
4. On timeout, persist one typed editor-route error with elapsed time and last observed
   lifecycle facts.

Acceptance criteria:

- no route path emits `route_dispatch_only_actor_state_direct_pane_submit` for an actor
  that has not produced a ready prompt in the current generation
- no follow-on `route_dispatch_only_starting_pane_not_ready` after a controller
  `dispatch_accepted`; acceptance and proof must agree before success is reported

## Phase 3: coalesce editor sync bursts and cache stable actor facts

Files: `src/agent-doc/src/sync.rs`, editor plugin sync callers

1. Re-open the sync-budget work for the current same-day evidence rather than treating
   the earlier phase as complete.
2. Coalesce duplicate safe-passive sync calls for the same focused file/window while a
   sync is already running.
3. Cache stable controller actor lookup results briefly for unchanged editor selection,
   invalidating on lifecycle transitions, file focus changes, and controller restart.
4. Keep full sync allowed to wait longer, but make safe-passive sync fail fast or join
   the in-flight operation rather than waiting around 3s on `sync_lock_wait`.

Acceptance criteria:

- warm safe-passive sync remains below 1s during editor selection churn
- repeated actor lookup over-budget entries are collapsed to one root-cause phase
- redundant sync requests log `joined`, `superseded`, or `skipped_stale`, not a long
  independent lock wait

## Phase 4: add one end-to-end tmux smoke and one deterministic schedule

Files: `src/agent-doc/src/route.rs`, `src/agent-doc/src/sync.rs`, SimWorld route/sync
tests

1. Add a deterministic schedule for `starting -> busy(restart_fresh_spawn) -> ready`
   that proves dispatch cannot occur before the current-generation ready prompt.
2. Add a small real-tmux smoke that starts a Codex-managed pane and immediately routes
   while it is still booting; the test should assert either eventual proven dispatch or
   a typed timeout, never ambiguous accepted-but-unproven success.
3. Add a sync burst schedule that reproduces the 3s lock-wait class without real tmux.

Acceptance criteria:

- `make check` exercises the deterministic route/sync schedules
- `make tmux-ci` keeps one live proof for the tmux prompt barrier
- failures print the document id, pane id, generation, actor state, last lifecycle
  reason, and elapsed wait

## Backlog order

1. Fix the typed `actor_binding` protocol first; it removes ambiguous controller state
   from both route and sync.
2. Fix startup route gating next; the current logs show routing can still race with a
   fresh Codex restart.
3. Then reduce sync churn; performance work is easier once absent/stale bindings are
   explicit.
