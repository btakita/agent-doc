# Plan: Stop hook loops after committed cycle is rewound to `write_applied`

## Problem

On April 30, 2026, `tasks/agent-doc/agent-doc-bugs2.md` entered the Codex Stop-hook recovery path even though the captured response had already crossed a commit boundary.

Evidence from the live session surfaces:
- `.agent-doc/captures/483039e7a9b103bd5b68223dac05a4c449ac03899aba53d6022e82100578b5b0/cycle-1777577852817.json` shows `state: "committed"` and `committed_at: 1777578017`.
- `.agent-doc/logs/b26b9957-8de6-4eae-bb98-5d7fe4d6b781.log` records `commit_success` for `cycle-1777577852817`, then immediately records `repair_applied`.
- `.agent-doc/state/cycles/483039e7a9b103bd5b68223dac05a4c449ac03899aba53d6022e82100578b5b0.json` was left at `phase: "write_applied"` / `last_event: "repair_applied"` after that later event.

That leaves the three closeout surfaces disagreeing:
1. capture says committed
2. session log says commit succeeded
3. cycle-state file says write landed but no terminal commit followed

The Stop hook keys off the open cycle state, so it continued to intercept the turn as unfinished.

## Desired behavior

Once a cycle has recorded terminal commit success, later repair bookkeeping must not rewind that same cycle back to an open phase.

## Investigation plan

- [ ] Trace the exact `preflight` / `repair` code path that records `repair_applied` after `commit_success` for the same cycle id.
- [ ] Define the intended precedence between cycle-state, capture-state, and session-log terminal events when they disagree.
- [ ] Make `repair_applied` idempotent on already-committed cycles, or synthesize a terminal closeout immediately after repair so the cycle file cannot stay at `write_applied`.
- [ ] Add regression coverage for the exact shape: committed capture + committed session log + later repair call on the same cycle must still leave `session-check` green and must not trigger the Stop hook.
