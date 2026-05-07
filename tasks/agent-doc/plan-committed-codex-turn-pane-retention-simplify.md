# Plan — simplify committed Codex turn pane retention

- [ ] Reconstruct the exact `monsterrodholders.md` failure chain from the local session log and freeze it as the primary repro:
  - `document_cycle ... event=commit_success`
  - `codex_exit code=0`
  - `ctrl_d_committed_cycle_restart_fresh`
  - successor `codex_start mode=fresh_restart`
  - successor `codex_exit code=0`
  - `ctrl_d_prompt_user`
  - `user_quit_after_ctrl_d`
  - `session_end`
- [ ] State the invariant in one sentence and enforce it everywhere:
  - after a Codex run commits a document cycle, no later supervisor branch created only to keep that pane alive may close the claimed tmux pane
- [ ] Remove the current multi-stage decision split for this case:
  - `CrashPolicy` clean-exit action
  - `restart_continue_exit_strategy(...)`
  - prompt-time EOF policy
  - later sync/rebind recovery after the pane is already gone
- [ ] Replace that split with one explicit post-commit keepalive mode:
  - the committed run may restart fresh once if needed
  - the successor run must not inherit the previous run's `Ctrl-D` quit authority
  - quitting after a committed turn must require an explicit new user action in the live successor pane, not leftover supervisor state
- [ ] Separate the three concepts that are currently conflated:
  - original child exited because stdin EOF / `Ctrl-D` was forwarded
  - successor keepalive run has not yet done useful work
  - user explicitly wants to close the pane
- [ ] Decide the simplest contract for editor-owned / routed Codex sessions:
  - either they never show the supervisor quit menu automatically
  - or they show it only on an explicit manual quit path from the live successor run
  - in either case, a successful routed turn must not fall into `user_quit_after_ctrl_d`
- [ ] Tighten diagnostics so the log makes one consistent claim:
  - one reason for the initial exit
  - one reason for the keepalive decision
  - one reason for any real pane close
  - no contradictory `restart_eval ... action=prompt_user` followed by `ctrl_d_committed_cycle_restart_fresh`
- [ ] Add regression coverage around the real repro instead of isolated helper cases:
  - committed Codex turn with forwarded `Ctrl-D` keeps the pane attached
  - the first successor keepalive run cannot hit `user_quit_after_ctrl_d` without fresh user input
  - routed/editor-owned sessions with no readable stdin fail closed by keeping the pane, not by quitting
  - sync must not need `registered_pane_missing` / `registry_rebind` churn to recover from this path because the pane should still exist
- [ ] Replay the live `src/boost-client/tasks/monsterrodholders.md` scenario after the change and verify:
  - the original pane remains the owner after a successful turn
  - no immediate replacement-pane churn starts
  - `session_end` happens only on an explicit quit or real destructive recovery path

## Simplification Goal

The current behavior is too stateful. A committed turn can still flow through a later supervisor branch that has permission to close the pane. The fix should simplify that contract so "committed turn" and "pane may close" are mutually exclusive unless the user explicitly asks to quit afterward.
