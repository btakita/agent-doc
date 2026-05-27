# Plan: JB Run Agent Doc fails after rescuing pane from stash — actor still Starting

**Bug class:** route/start race after stash rescue
**Related:** `plan-route-starting-actor-race-after-restart.md`, `plan-stashed-pane-silent-death.md`, `plan-passive-sync-existing-pane-handoff.md`
**Severity:** medium — JB Run Agent Doc fails closed after silent stash recovery

## Problem

JB plugin "Run Agent Doc" on `tasks/software/tsift.md` produces:

```
resync: stash pane %11 is registered in its own project root — skipping kill
[route] debounce OK — file idle for 26.2s and typing indicator idle
[route] target tmux session: 0
[route] Pane %33 is in stash window 'stash', rescuing to agent-doc window
[route] Rescued pane %33 via join-pane
Error: authoritative actor generation 9 for tasks/software/tagpath.md owns
pane %33 but route will not inject a new trigger because the authoritative
actor is still starting (waited 8s for codex startup). Wait for the pane to
show a dispatch-ready prompt (`prompt_ready=true`), then rerun ...
```

Two distinct issues:

1. **Cross-file pane ownership leak:** Run Agent Doc was invoked for
   `tsift.md`, but the rescued pane `%33` is owned by `tagpath.md` (generation
   9). The route should refuse before mutating cross-file pane state, not
   after.
2. **Starting-actor wait too short after rescue:** Even if pane ownership is
   correct, the 8s codex startup wait after a fresh rescue (where the pane
   may have been suspended/backgrounded) is insufficient. Rescue-from-stash
   should reset / extend the startup budget.

### Root cause hypotheses

- `route_resync` rescues a stash pane based on tmux topology without verifying
  the target file matches the authoritative actor's owning document.
- `rescue_pane_via_join` reuses the existing actor generation and
  `Starting`/`Ready` state rather than re-promoting after rescue, so the
  freshly visible pane carries stale boot-state metadata.
- Codex startup budget (8s) was tuned for fresh `agent-doc start`, not for
  rescue scenarios where the harness may need to redraw prompts.

## Plan

### Phase 1: Refuse cross-file rescue at route gate (binary)

**File:** `src/route.rs`, `src/resync.rs`

1. Before `rescue_pane_via_join`, compare the route target file against the
   pane's authoritative actor's owning document.
2. On mismatch, refuse rescue with a clear error citing both files and stop;
   do not move the pane.
3. Emit ops-log event `route_cross_file_rescue_refused`.

### Phase 2: Reset Starting wait after rescue

**File:** `src/route.rs`

1. After a successful rescue-from-stash, treat the actor as fresh-Starting and
   apply the harness-aware startup budget defined in
   `plan-route-starting-actor-race-after-restart.md`.
2. If the pane was rescued but the actor was already Ready before stash, skip
   the wait and emit `route_rescued_already_ready`.

### Phase 3: Rescue diagnostics

**File:** `src/route.rs`, `src/ops_log.rs`

1. Log `route_rescue_source_window`, `route_rescue_target_window`,
   `route_rescue_actor_state`, `route_rescue_owning_file`.
2. Include rescue context in the "still starting" error message so the user
   knows the wait was applied after a rescue.

### Phase 4: Regression coverage

**File:** tests / SimWorld

1. SimWorld: stash a pane owned by file A, invoke route for file B — assert
   cross-file refusal and no pane movement.
2. SimWorld: rescue a stash pane owned by the target file with actor in
   Starting — assert extended wait and successful dispatch when the harness
   becomes ready.
3. Unit: route refusal message includes the owning file name.

## Testing

- JB Run Agent Doc on `tsift.md` while stash holds a pane owned by
  `tagpath.md` → no rescue, clear error citing both files.
- JB Run Agent Doc on `tsift.md` with own stashed pane in Starting → wait
  applies, dispatch proceeds when harness is ready.
- `agent-doc session status <file>` after refusal still reports the pane as
  stashed and untouched.
