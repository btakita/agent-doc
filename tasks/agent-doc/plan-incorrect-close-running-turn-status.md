# Incorrect Session Close While Prior Turn Still Running

## Problem

Two agent-doc-managed code sessions were closed or superseded while adjacent managed panes still had live agent processes. The user asked whether the second previous turn was still running. Live inspection showed:

- `src/boost-client/tasks/monsterrodholders.md` had a live OpenCode pane, but the turn had completed and the pane was idle at the OpenCode prompt.
- `src/session-share/tasks/root.md` had a live Claude pane that still appeared to be mid-turn, with child exploration work visible in the TUI.

The confusing part is that session registry state and live pane state can disagree during route-owned handoff, clear-context, and closeout recovery. Users need a reliable answer before agent-doc reuses, clears, or reaps a pane.

## Evidence

- Root project process table showed current `tasks/agent-doc/agent-doc-bugs2.md`, previous `tasks/root.md`, and older `src/boost-client/tasks/monsterrodholders.md` managed sessions alive at the same time.
- Boost-client `.agent-doc/session-actors.json` recorded `tasks/monsterrodholders.md` generation 216 as `ready`.
- `tmux capture-pane -t %39` showed the MonsterRodHolders OpenCode turn had completed the deploy/cache-purge work and returned to the OpenCode prompt.
- `tmux capture-pane -t %35` showed the `root.md` Claude pane still in a live turn with active exploration output.

## Fix Plan

1. Add a `agent-doc status --live <FILE>` or equivalent diagnostic that reports both projected actor state and direct pane evidence: process alive, pane current command, recent output tail, and whether a harness prompt is visible.
2. Teach closeout/reap paths to distinguish `alive-idle`, `alive-busy`, `closed-clean`, and `projection-stale` instead of relying on one registry state.
3. Before any clear/restart/reap on a route-owned actor, fail closed if direct pane evidence indicates an active turn or active child-agent/subtask output.
4. Add regressions for the observed pattern: OpenCode pane completed but left alive at prompt, while a neighboring Claude pane is still mid-turn.
5. Surface the result in user-facing diagnostics so a user can tell whether “session exists” means “turn still running” or only “pane still alive.”

## Verification

- SimWorld/unit coverage for projected-ready + pane-busy mismatch.
- tmux-backed smoke test for an idle OpenCode prompt after route-owned completion.
- tmux-backed smoke test for a Claude pane with active child work that must not be reaped or reused.
