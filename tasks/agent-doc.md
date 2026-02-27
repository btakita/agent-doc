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

## Assistant

Updated `~/.config/tmux/.tmux.basic.conf` — added 3 settings with `# agent-doc:` comment block:

```bash
# agent-doc: reduce flicker during pane operations and high-throughput output
set -sg escape-time 10                              # fast escape (default 500ms causes lag)
set -g focus-events on                              # consistent redraw on focus change
set -ga terminal-overrides ",xterm-256color:Tc"     # true color support, fewer escape redraws
```

**What was already set:** `escape-time 10` was already active at runtime (from tmux-sensible plugin). Added it to the config file for persistence.

**What changed:** `focus-events on` and `terminal-overrides` with `Tc` (true color) — both applied to the running session immediately.

Switch between some task documents to see if it helped. The tab-switch flickering will need the command batching fix in agent-doc itself for full resolution — these tmux settings mainly help with the diff-display flickering.

## User

Still getting these not found errors:

```
● Bash(cp tasks/corky.md .agent-doc/snapshots/f2df7f9ef91ea80a49568ee9cffa2966c1dcec6813ffb86c134e14e2273e28c3.md)
  ⎿  Error: Exit code 1
     cp: cannot stat 'tasks/corky.md': No such file or directory

     cp: cannot stat 'tasks/corky.md': No such file or directory

● Bash(pwd && ls tasks/corky.md 2>&1)
  ⎿  Error: Exit code 2
     /home/brian/work/btakita/agent-loop/corky
     ls: cannot access 'tasks/corky.md': No such file or directory

     /home/brian/work/btakita/agent-loop/corky
     ls: cannot access 'tasks/corky.md': No such file or directory

● Bash(cp /home/brian/work/btakita/agent-loop/tasks/corky.md
      /home/brian/work/btakita/agent-loop/.agent-doc/snapshots/f2df7f9ef91ea80a49568ee9cffa2966c1dcec6813ffb86c1…)
  ⎿  Done

✽ Percolating… (4m 9s · ↑ 4.5k tokens · thought for 2s)
```
