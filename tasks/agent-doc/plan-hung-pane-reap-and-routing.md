# Plan — hung pane reaping and startup routing miss

**Pending:** `#panehang`
**Status:** Draft

## Problem

Two distinct failure modes leave Claude Code panes idle in tmux after `agent-doc start`:

### Bug 1: Post-completion pane not reaped

Pane 4:1.0 (%256, PID 2165040) completed its agent-doc cycle (processed tasks, ran "push"), but the Claude Code instance stayed alive at the idle prompt instead of exiting. The pane shows `ctx:25%` and the last operation was a successful push — the cycle is done, but nothing closed the pane.

**Root cause hypothesis:** `agent-doc start` spawns Claude Code in a tmux pane and injects the `/agent-doc <FILE>` command, but after the cycle completes (finalize + session-check pass), there is no exit signal sent to the Claude instance. The harness treats the pane as a persistent workspace, but for `agent-doc start`-spawned panes (as opposed to user-created panes), the expected lifecycle is: spawn → route → process → finalize → exit.

### Bug 2: Startup routing miss — no command injected

Pane 4:1.1 (%271, PID 3176953) was spawned by `agent-doc start`, loaded the SessionStart hook, but never received the `/agent-doc <FILE>` routing command. It sits at the default "Try..." prompt indefinitely.

**Root cause hypothesis:** The `agent-doc start` command registered the session and launched Claude Code, but the routing step that injects the `/agent-doc <FILE>` command into the pane failed silently — possibly a race condition between pane creation and command injection, or the IPC routing path dropped the message.

## Evidence

```
# Pane 4:1.0 — completed work, still alive
tmux capture-pane -t 4:1.0 → shows "push" as last command, idle prompt, ctx:25%

# Pane 4:1.1 — never received routing
tmux capture-pane -t 4:1.1 → shows Claude Code startup banner + SessionStart hook, idle "Try..." prompt

# Pane IDs
4:1.0 = %256, PID 2165040
4:1.1 = %271, PID 3176953
```

## Desired Behavior

1. `agent-doc start`-spawned panes should exit after the agent-doc cycle completes (finalize + session-check pass).
2. If routing injection fails, the spawned pane should either retry routing or exit with a diagnostic message — never sit idle.
3. Stale idle panes from completed or failed cycles should be detected and reaped by `agent-doc sync` or `agent-doc doctor`.

## Implementation Steps

### Phase 1: Post-completion exit for start-spawned panes

1. Add a `spawned_by=start` flag to the session actor record when `agent-doc start` creates the pane.
2. After `finalize` / `session-check` completes successfully in a `spawned_by=start` pane, send an exit command to the Claude instance (e.g., `/exit` or tmux `send-keys` with the exit sequence).
3. If the cycle fails, leave the pane alive for debugging but log a diagnostic.

### Phase 2: Routing injection reliability

1. After `agent-doc start` creates the pane and registers the session, add a readiness gate: wait for the Claude Code prompt to appear (poll pane content for the prompt indicator) before injecting `/agent-doc <FILE>`.
2. Add a timeout (e.g., 30s) — if the prompt never appears, log an error and kill the pane.
3. After injection, verify the command was accepted (e.g., check pane content for skill invocation output within 10s).

### Phase 3: Stale pane detection

1. In `agent-doc sync` / `agent-doc doctor`, detect panes that:
   - Have `spawned_by=start` and no active cycle for >60s
   - Show the idle Claude Code prompt (no in-progress work)
2. Offer to reap these panes in `doctor --repair`, log them in `doctor` diagnostics.
