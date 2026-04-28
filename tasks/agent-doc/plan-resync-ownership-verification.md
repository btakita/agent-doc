# Plan — resync missing ownership verification (#resyncown)

**Pending:** `#resyncown`
**Status:** Draft

## Problem

`route` requires a **live file-owning process tree** before reusing a registered pane — if no process in the pane's tree mentions the document path (via argv or supervisor PID), route fails closed with:

```
registered pane %283 is alive but no live agent-doc session currently
owns <file>. Refusing to route into an ambiguous pane; restart or
reclaim the session instead
```

`resync` does **not** perform this same check. It only verifies:
- Pane alive?
- Pane in stash?
- Running an agent process?
- Correct tmux session?

So a pane that is alive, not in stash, shows `agent-doc` as `current_command`, and is in the right tmux session passes `resync` cleanly — but `route` blocks on it because no process actually owns the file. The user is stuck: `resync --fix` reports "no issues" while `route` refuses to proceed.

**Reproduction (2026-04-28):** Pane %283 was registered for `tasks/claudescore-3.md`. The pane was alive running a Claude Code shell (PID was `-zsh`). `resync` reported it healthy. `route` failed closed. Manual `sessions.json` edit was required.

## Root Cause

`detect_issues_in_registry()` in `resync.rs` (~line 1060) classifies pane health from tmux metadata (alive, process name, session, window) but never calls `sync::find_alive_pane_for_file()` or checks the supervisor socket for a live child. Route's ownership proof (route.rs ~477-529) is strictly stronger than resync's health check, creating a state that resync considers healthy but route considers ambiguous.

## Desired Behavior

1. `resync` (dry-run) reports panes where route would fail closed — a new issue type like `NoLiveOwner` alongside existing `InStash`, `WrongProcess`, `WrongSession`.
2. `resync --fix` deregisters `NoLiveOwner` entries (the pane is alive but unowned, so kill is not appropriate — just remove the stale registration).
3. `route` auto-recovers from `NoLiveOwner` instead of only failing closed: deregister the stale entry, then fall through to Strategy 2 (scan for unregistered live pane) or Strategy 3 (start fresh).
4. Stash panes with `--fix` that are unregistered and have no live owner should be killable, not skipped as "may be rescuable."

## Implementation Steps

### resync: add NoLiveOwner detection

- [ ] In `detect_issues_in_registry()`, after the existing alive-pane checks pass (not in stash, correct process, correct session), call `sync::find_alive_pane_for_file()` + supervisor PID fallback for the entry's `file` field.
- [ ] If neither proof finds a live owner, emit a new `NoLiveOwner` issue variant with the entry's pane, file, and PID.
- [ ] In `apply_fixes_to_registry()`, handle `NoLiveOwner` by deregistering the entry (no pane kill — the shell is alive and may be the user's).
- [ ] In dry-run output, surface it clearly: `pane %283 registered for tasks/claudescore-3.md but no live agent-doc session owns the file — would deregister`

### resync: tighten stash pane cleanup

- [ ] With `--fix`, unregistered stash panes that have no live file owner should be killed, not skipped with "may be rescuable." Only skip if the pane has a provable live owner for some file.

### route: auto-recover from stale registration

- [ ] When route's ownership proof finds no live owner and the registered pane is alive, instead of immediately failing closed:
  1. Deregister the stale entry (call `sessions::deregister()`)
  2. Log the deregistration with pane provenance
  3. Fall through to Strategy 2 / Strategy 3 (scan unregistered panes or start fresh)
- [ ] Only fail closed if the fallback strategies also cannot resolve ownership.

### Tests

- [ ] Regression: registered pane alive, running shell (no file in process tree), no supervisor socket → resync reports `NoLiveOwner`, `--fix` deregisters
- [ ] Regression: route with same state auto-deregisters and falls through to start fresh
- [ ] Regression: stash pane with no live owner is killed by `resync --fix`
- [ ] Regression: stash pane with a provable live owner is preserved by `resync --fix`

### Spec updates

- [ ] `specs/07-commands.md` — document `NoLiveOwner` issue type for `resync`, and route's auto-recovery behavior
- [ ] `specs/supervisor.md` — note that supervisor socket is consulted by both route and resync for ownership proof

## Files Likely To Change

- `src/agent-doc/src/resync.rs` — `detect_issues_in_registry()`, `apply_fixes_to_registry()`, stash cleanup
- `src/agent-doc/src/route.rs` — auto-recovery fallthrough in Strategy 1 fail-closed path
- `src/agent-doc/src/sessions.rs` — possible `deregister()` helper if not already exposed
- `src/agent-doc/specs/07-commands.md`
- `src/agent-doc/specs/supervisor.md`

## Exit Criteria

- `resync` and `route` use the same ownership proof — no state can pass resync but block route
- A stale registration with a live-but-unowned pane is auto-recoverable without manual sessions.json editing
- Stash panes without live owners are cleaned up by `resync --fix`
