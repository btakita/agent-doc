# Plan — visible tmux pane loses ownership proof and gets replaced

**Pending:** `#ownergap`
**Status:** Draft

## Problem

The real bug is not merely that JetBrains selection sync can auto-start. The more fundamental failure is that `agent-doc` can look at a tmux pane the user can still see running, fail to prove that it still owns the document, and then replace it with a fresh pane era.

Concrete evidence from the current code and persisted `monsterrodholders.md` logs:

- `src/boost-client/.agent-doc/logs/a9421282-fd96-4943-9af5-3561ed5cb799.log` shows repeated `session_superseded` and `session_end origin=registry_rebind` events rather than true child-crash provenance.
- The same log later records sync-driven `supervisor_exit code=missing_pane ... reason=registered_pane_missing` for visible-pane eras like `%71`, followed by immediate replacement starts on `%72` and `%73`.
- In `src/agent-doc/src/sync.rs`, `find_live_owner_pane...` only proves ownership through:
  - registry path provenance: registered pane + matching supervisor pid + matching supervisor instance id
  - process-tree file-path match
  - supervisor-pid fallback
- `find_associated_panes()` only accepts candidates with `ProcessTree` or `SupervisorPid` proof; a merely registered visible pane is discarded if those proofs are absent.
- `find_alive_pane_for_file_inner()` / `collect_process_tree_matches()` only inspect the pane pid and its direct children for `agent-doc` + file-path-bearing argv. If the live pane is still real but the foreground process tree no longer advertises the file path in that narrow shape, the process-tree proof disappears.
- Once both the process-tree proof and supervisor socket proof are missing, sync falls through to `repair_missing_registered_pane(...)`, records synthetic `registered_pane_missing`, and makes replacement legal even though the pane may still be visibly alive.

So the root issue is a proof gap: visible pane existence is not enough, and the stronger ownership proofs are currently narrow enough that a real owner can temporarily fall through them and be treated as missing.

## Desired Behavior

1. A visible pane that still corresponds to the latest open session era for the document must not be replaced just because file-path argv proof disappeared transiently.
2. Ownership proof must degrade more gracefully than the current binary yes/no model.
3. Before recording synthetic `registered_pane_missing`, sync/start/route must exhaust broader provenance that can prove a live pane is still the current owner.
4. If ownership is still ambiguous after stronger recovery, fail closed with diagnostics instead of replacing the pane.

## Implementation Steps

- [ ] Reconstruct the exact proof failure chain for the `monsterrodholders.md` visible-pane repro:
  - what pane was visible
  - which proof branch failed first: path provenance, process-tree match, or supervisor pid
  - whether the supervisor socket was actually gone, stale, or just temporarily unreachable
- [ ] Audit the current proof surfaces in `sync.rs`, `route.rs`, and `start.rs`:
  - `find_registered_pane_via_path_provenance`
  - `find_alive_pane_for_file_inner`
  - `collect_process_tree_matches`
  - `find_alive_pane_via_supervisor_pid`
  - the `NoSocket` / `Unreachable` route/start branches
- [ ] Decide the missing intermediate proof layer for a visible pane:
  - session-log latest-open-owner proof
  - deeper descendant traversal than direct children
  - supervisor ancestry / restarted-child provenance
  - pane-local cwd / project-root correlation combined with latest session log
- [ ] Tighten the replacement boundary:
  - `registered_pane_missing` should only be recorded after the stronger visible-owner checks fail
  - an alive pane with recent open-session provenance should fail closed or be reused, not replaced
- [ ] Add explicit ops/session-log provenance for proof failure:
  - which proof strategies were attempted
  - why each failed
  - whether the pane was still tmux-alive and visible
  - why replacement was allowed anyway
- [ ] Add regression coverage for:
  - visible pane alive, no argv file-path match, supervisor socket temporarily unavailable
  - visible pane alive, latest session log still open, replacement must not happen
  - ambiguous multiple visible candidates must fail closed instead of replacing

## Files Likely To Change

- `src/agent-doc/src/sync.rs`
- `src/agent-doc/src/route.rs`
- `src/agent-doc/src/start.rs`
- `src/agent-doc/src/startup_miss.rs`
- `src/agent-doc/src/session_check.rs`
- `src/agent-doc/specs/07-commands.md`

## Exit Criteria

- A visibly alive pane for `monsterrodholders.md` is no longer replaced merely because the narrow live-owner proof vanished.
- `registered_pane_missing` is only emitted after stronger visible-owner proof exhaustion.
- Ownership-proof failure surfaces enough provenance to explain exactly why a replacement was or was not allowed.
