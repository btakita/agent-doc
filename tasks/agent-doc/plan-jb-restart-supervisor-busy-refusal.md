# Plan: JB Restart Supervisor Process refused by alive-busy guard

**Bug class:** JB plugin / session_restart UX
**Related:** [#jbccfix], `plan-jb-clear-session-busy-actions.md`, `plan-claudescore-3-busy-reroute-interrupt-validation.md`
**Severity:** medium — Restart Supervisor is the user's escape hatch and fails closed when most needed

## Problem

JetBrains plugin action "Restart Supervisor Process" calls
`agent-doc session_restart <FILE>`, but the binary refuses with:

```
Error: session_restart refused for tasks/software/tsift.md because pane %13
is alive-busy (source=authoritative_actor, current_command=agent-doc,
tail="gpt-5.5 xhigh · ~/work/btakita/agent-loop · Context 30% used").
Run `agent-doc session status ...` and wait for an idle prompt, or
inspect/stop the pane explicitly before clearing or restarting it.
```

The Restart Supervisor action is the user's explicit "kick this loose" gesture.
A blanket alive-busy refusal turns the intended escape hatch into a dead end.

### Root cause hypotheses

1. `session_restart` shares the same alive-busy gate as `clear` / `auto`-style
   transitions; restart-intent is not distinguished from non-destructive
   transitions.
2. The JB action passes no `--force` / `--interrupt` flag, so the binary cannot
   tell the user explicitly opted into killing a busy turn.
3. JB does not pre-interrupt the pane (no Ctrl-C / harness-specific kill)
   before issuing `session_restart`, so the busy state persists.

## Plan

### Phase 1: Distinguish restart-intent from passive transitions (binary)

**File:** `src/session_restart.rs` (or equivalent restart entrypoint), `src/cli.rs`

1. Add `agent-doc session_restart <FILE> --force` accepting alive-busy panes;
   keep the bare command's current fail-closed behavior.
2. Plumb a `restart_intent: explicit` source through the alive-busy guard so the
   refusal message can hint at `--force`.
3. Refusal message: append "Pass `--force` to interrupt the running turn and
   restart anyway." when `--force` was not supplied.

### Phase 2: JB plugin sends force + visible confirmation

**File:** `editors/jetbrains/` Restart Supervisor action handler

1. When the user clicks Restart Supervisor and the pane is busy, prompt:
   "Supervisor is busy on a running turn. Interrupt and restart?" (Yes/No).
2. On confirm, invoke `agent-doc session_restart <FILE> --force`.
3. Surface restart progress (`Restarting...`) in the IDE notification area; do
   not silently swallow stderr.

### Phase 3: Pre-restart interrupt step (optional, harness-aware)

**File:** binary's restart path

1. Before killing the supervisor, send harness-appropriate interrupt
   (Ctrl-C / `agent-doc cancel` / harness escape sequence) and wait up to 2s
   for idle.
2. If the pane reaches idle, proceed with non-force restart automatically.
3. If still busy, fall through to `--force` behavior when the flag is set.

### Phase 4: Telemetry + regression coverage

**File:** `src/ops_log.rs`, tests

1. Log `session_restart_force_used`, `session_restart_busy_pre_interrupt_idle`,
   and `session_restart_busy_force_killed` distinct events.
2. SimWorld scenario: route + Restart Supervisor while pane is alive-busy with
   and without `--force`, asserting the new refusal message and force success.
3. Unit test for the refusal-message text so the `--force` hint stays present.

## Testing

- `agent-doc session_restart <FILE>` against alive-busy pane → fails with new
  message including `--force` hint.
- `agent-doc session_restart <FILE> --force` against alive-busy pane → kills
  busy turn, restarts supervisor, exits 0.
- JB plugin Restart Supervisor → prompt + force path executes; idle pane path
  unaffected.
