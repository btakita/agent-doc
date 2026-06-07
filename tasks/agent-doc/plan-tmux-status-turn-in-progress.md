# Plan: tmux status line should show "turn in progress" during an active turn

`#tmux-status-turn-in-progress`

## Symptom (live, 2026-06-06)

User: *"I'm running a turn on monsterrodholders.md. The tmux status is blank. I
should be 'agent-doc: turn in progress'."*

While a managed agent turn is actively running, the tmux status line for the
owned pane/window shows nothing, so the operator cannot tell at a glance that the
session is busy vs idle.

## Fix direction

1. Surface the actor lifecycle state on the tmux status line: when the
   authoritative actor is `busy` (turn in progress) show e.g.
   `agent-doc: turn in progress` (or a per-document variant), and clear/blank it
   back to an idle indicator when the actor returns to `ready`.
2. The supervisor already owns the busy→ready transitions
   (`transition_actor_state` in `start.rs`) and now also self-heals stale-busy via
   the idle-queue watch (`#stale-busy-after-auto-inject-no-clear`). Hook the
   status-line update to those same transition points so the indicator can never
   diverge from the actor state. Prefer driving it from the supervisor (single
   source of truth) over a separate poller.
3. Mechanism: tmux `set-option -p`/`-w` status or `display-message`-style pane
   title; reuse the existing managed-status surface (`start.rs` already pushes
   `[start] managed ... capability proof` via tmux `display-message` on the owned
   pane — keep these diagnostics out of the child pane stdout per the project
   rule). Decide pane-status vs window-status placement.
4. Clear the indicator on turn completion, supervisor stop, and pane close so a
   stale "turn in progress" cannot linger (same failure class the stale-busy
   self-heal addresses for the actor projection).

## Related

`#claude-busy-status-during-active-turn` (review: surface a visible
turn-in-progress busy status on the owned pane), `#jb-stale-busy-idle-footer`,
`#stale-busy-after-auto-inject-no-clear`. Overlaps the "busy status during active
turn" theme — coordinate so the tmux indicator and the actor-state self-heal use
one transition source.
