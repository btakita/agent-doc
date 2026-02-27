---
session: 56f0c92a-f2e9-44d2-9e7b-872ec3e14d91
resume: null
agent: null
model: null
branch: null
---

## User

I see flickering in the tmux when switching task documents.

There is also flickering when the claude session shows diffs.

## Assistant

Two separate flickering issues:

### 1. Tab-switch flickering — fixable

**Root cause:** Sequential tmux commands without batching. `select_pane()` spawns two processes (`select-window` + `select-pane`). `layout()` runs `break-pane` and `join-pane` in loops — each forces a separate tmux render cycle.

**Fix:** Batch with `\;` command chaining:
```bash
# One process, one render cycle:
tmux select-window -t %5 \; select-pane -t %5 \; break-pane -s %3 -d \; join-pane -s %7 -t %5 -h
```

**Also:** Plugin debounce is 100ms (too short for rapid tab switches) and has no concurrency guard (overlapping `agent-doc` processes race on tmux state).

**Proposed changes:**
1. `TmuxBatch` builder in `sessions.rs` — collects commands, single `tmux` invocation
2. Refactor `select_pane()` and `layout()` to use batch
3. Plugin debounce 100ms → 250ms
4. `AtomicBoolean` lock to prevent overlapping executions

### 2. Diff-display flickering — tmux rendering characteristic

This happens when Claude streams large diffs to the terminal. Tmux redraws the pane rapidly as output arrives. Not an agent-doc bug — it's how tmux renders high-throughput output.

**Mitigations to try in your tmux config:**
```bash
set -sg escape-time 10          # reduce input delay (default 500ms)
set -g focus-events on           # consistent redraw on focus
set -g terminal-overrides "xterm*:Tc"  # true color, fewer escape redraws
```

If the flickering is specifically in *other* panes (not the one showing the diff), that would point to layout sync triggering during Claude output — the batch fix from issue 1 would help there too.

Want me to implement the batch fix?

## User

Please update my tmux config...add surrounding comments. If this works, we should document.
