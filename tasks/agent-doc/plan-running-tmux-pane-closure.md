# Plan — running tmux panes must not be closed by agent-doc

**Pending:** `#rnpane`
**Status:** Draft

## Problem

User report: agent-doc closed the running tmux pane.

Treat this as a product-contract failure, not a normal recovery bug. A pane that is still the authoritative owner for a document must not disappear because a turn committed, because a keepalive/supervisor branch misread inherited EOF, or because a later sync/layout pass decided the pane was disposable.

The current evidence points to two real failure families, not one vague class of "pane loss":

1. **Committed-turn supervisor closeout still reaches a quit path.**
   In `.agent-doc/logs/b26b9957-8de6-4eae-bb98-5d7fe4d6b781.log`, several `tasks/agent-doc/agent-doc-bugs2.md` Codex panes cleanly exit with `codex_exit code=0`, then still flow into `restart_eval ... action=prompt_user` and immediate closeout events such as:
   - `%260`: `user_quit_after_eof`
   - `%271`: `auto_restart_clean` -> `resume_restart_failed` -> fresh restart -> `user_quit_after_eof`
   - `%355` / `%356`: `ctrl_d_prompt_user` -> `user_quit_after_ctrl_d`

2. **A later routed/sync/layout pass can remove a pane without a matching session closeout.**
   In `.agent-doc/logs/ops.log`, the later `%169` repro for `tasks/agent-doc/agent-doc-bugs2.md` shows repeated `commit_success` and `route_dispatch_only_sent` events, but no matching `session_end`, `supervisor_exit`, or `session_superseded` for `%169` at the point the pane was reported gone. That makes the disappearance look more like post-completion tmux mutation than true child exit.

So the fix has to audit the full pane-lifecycle surface and define one hard invariant:

> A running or just-committed `agent-doc` pane may leave its visible tmux position only through a short explicit allowlist of destructive actions, each with concrete provenance.

## Desired Behavior

1. A live claimed pane is never closed implicitly at turn end.
2. A later `sync`, editor event, route rerender, or layout reconcile cannot stash, replace, reap, or orphan the sole authoritative pane from weak evidence.
3. If agent-doc does move or destroy a pane, the session log and ops log must record which subsystem initiated it and why.
4. Ambiguous ownership must fail closed and preserve tmux state instead of mutating it.

## Primary Repro Seeds

### A. Supervisor / keepalive closeout repro

- Session: `b26b9957-8de6-4eae-bb98-5d7fe4d6b781`
- Document: `tasks/agent-doc/agent-doc-bugs2.md`
- Evidence:
  - clean `codex_exit code=0`
  - then `restart_eval ... action=prompt_user`
  - then `user_quit_after_eof` or `user_quit_after_ctrl_d`
  - then `session_end`
- Interpretation:
  - a successful or effectively successful run can still inherit quit authority into a later supervisor branch and close the pane without new user intent

### B. Post-commit tmux mutation repro

- Document: `tasks/agent-doc/agent-doc-bugs2.md`
- Pane: `%169`
- Evidence from `.agent-doc/logs/ops.log`:
  - repeated `commit_success`
  - repeated `route_dispatch_only_sent file=tasks/agent-doc/agent-doc-bugs2.md pane=%169 harness=codex`
  - no matching `session_end`, `supervisor_exit`, or `session_superseded` for `%169` when the pane was later observed missing
- Interpretation:
  - the pane likely remained the owner and was later moved, stashed, or replaced by route/sync/layout logic rather than the supervisor closeout path

## Investigation Scope

- Freeze both repro families with exact tmux/session context:
  - document path
  - session UUID
  - pane id
  - tmux session/window before and after disappearance
  - session-log events near the disappearance
  - first `sync`, `route`, `resync`, or DETACH event after the last known healthy turn
- Classify the actual boundary for each repro:
  - explicit supervisor quit / `session_end`
  - `registry_rebind` or replacement handoff
  - tmux-router DETACH / stash
  - dead-pane cleanup / missing-pane recovery
  - window-level tmux destruction after pane motion
- Compare the failure against adjacent backlog items so fixes do not fragment:
  - `#jbpdrop`
  - `#tailpatch`
  - `#rspcmt`
  - any older `monsterrodholders.md` pane-retention plans already in `tasks/agent-doc/`

## Destructive Surface To Audit

The code search already shows the concrete mutation primitives that can make a pane "disappear", even when it is not literally killed:

- `src/agent-doc/src/start.rs`
  - supervisor quit / restart / inherited EOF branches
- `src/agent-doc/src/route.rs`
  - `split-window`
  - rescue `join-pane`
  - stash replacement / eviction helpers
  - remaining direct `kill-pane` test and repair paths
- `src/agent-doc/src/sync.rs`
  - missing-pane repair
  - stash consolidation via `join-pane`
  - secondary stash-window cleanup via `kill-window`
- `src/agent-doc/src/resync.rs`
  - unregistered stash purge
  - dead non-stash purge
  - redundant stash-pane cleanup
  - explicit `kill-pane` / `kill-window` maintenance paths
- `src/tmux-router/src/sync.rs`
  - DETACH phase calls through `stash_pane(...)`
- `src/tmux-router/src/tmux.rs`
  - the shared low-level `kill_pane(...)` and `stash_pane(...)` primitives

## Implementation Plan

- [ ] Freeze the two primary repros into tmux-backed regression seeds before changing behavior.
- [ ] Define the destructive-action allowlist in one place:
  - explicit user-requested quit
  - explicit repair command that the user intentionally invoked
  - proven duplicate-pane reap
  - proven dead-pane cleanup with captured provenance
  - explicit handoff after a logged `session_superseded` / `session_end origin=registry_rebind`
- [ ] Fail closed everywhere else:
  - a committed-turn keepalive branch must not inherit prior EOF / `Ctrl-D` quit authority
  - route/sync/layout must not move or stash the sole authoritative pane from weak evidence
  - passive reconcile must preserve a visible pane even if ownership proof is incomplete
- [ ] Centralize pane-loss diagnostics:
  - document
  - session UUID
  - pane id
  - mutation primitive (`kill-pane`, `kill-window`, `stash_pane`, `join-pane`, `split-window`)
  - source subsystem
  - before/after window ids
  - whether the action was allowlisted
- [ ] Add tmux-backed regression coverage for:
  - clean committed Codex turn does not close the pane
  - fresh keepalive successor cannot hit `user_quit_after_ctrl_d` or `user_quit_after_eof` without new user input
  - post-commit route/sync/layout cannot stash or relocate the only authoritative pane
  - DETACH respects an `agent-doc` protected-pane contract for running or just-committed panes
  - ambiguous ownership preserves the pane and fails closed

## Likely Files

- `src/agent-doc/src/start.rs`
- `src/agent-doc/src/route.rs`
- `src/agent-doc/src/sync.rs`
- `src/agent-doc/src/resync.rs`
- `src/tmux-router/src/sync.rs`
- `src/tmux-router/src/tmux.rs`
- `src/agent-doc/specs/supervisor.md`
- `src/agent-doc/specs/07-session-tmux-commands.md`
- `src/agent-doc/specs/08-session-routing.md`
- `src/agent-doc/README.md`

## Exit Criteria

- A running or just-committed `agent-doc` pane no longer disappears without explicit user action or an explicit repair command.
- Any remaining pane-destroying or pane-relocating action is on a short documented allowlist and emits concrete provenance.
- Regression coverage proves pane retention across supervisor, route, sync, resync, and tmux-router DETACH boundaries.
