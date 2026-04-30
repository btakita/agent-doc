# Plan — claimed pane crashes when editor navigates to the document

**Pending:** `#edcrash`
**Status:** Draft

## Problem

When the user's editor (JetBrains IntelliJ IDEA) navigates to `tasks/claudescore.md`, the tmux pane that claimed the document crashes. The session log for session `143cbb97` shows 9+ consecutive `session_start` entries (panes %416 through %510) with no matching `claude_exit` or `session_end`. The supervisor process is killed mid-flight when the editor's tmux-router sync replaces the pane.

**Root cause hypothesis:** The editor fires `tmux-router sync --focus <file>` on buffer navigation. The sync operation may stash, kill, or replace the pane that currently runs the `agent-doc start` supervisor. Because pane termination happens at the tmux layer (not via IPC or signal to the supervisor), the supervisor can't log its exit, clean up cycle state, or transfer ownership — it simply vanishes.

**Evidence from the session log:**
```
[1777496369] session_start file=tasks/claudescore.md pane=%450 ... claude_start mode=fresh restart_count=0
[1777496729] session_start file=tasks/claudescore.md pane=%454 ... claude_start mode=fresh restart_count=0
[1777497966] session_start file=tasks/claudescore.md pane=%457 ... claude_start mode=fresh restart_count=0
[1777498998] session_start file=tasks/claudescore.md pane=%467 ... claude_start mode=fresh restart_count=0
[1777504276] session_start file=tasks/claudescore.md pane=%501 ... claude_start mode=fresh restart_count=0
[1777504781] session_start file=tasks/claudescore.md pane=%510 ... claude_start mode=fresh restart_count=0
```

No `claude_exit`, `user_quit`, or `session_end` between starts. Each pane is a fresh spawn with `restart_count=0` — the supervisor didn't restart; the entire process tree was replaced.

## Fix Plan

- [ ] Reproduce the crash by navigating to a claimed document in the editor while `agent-doc start` owns the pane. Confirm the pane death mechanism (tmux `kill-pane` vs stash vs respawn).
- [ ] Investigate the editor → tmux-router integration path: which autocmd / plugin / keybinding fires `tmux-router sync` on file navigation, and does sync actually kill the claimed pane or does the editor replace it directly?
- [ ] Add a signal handler (SIGTERM/SIGHUP) to the `agent-doc start` supervisor so it can log a clean exit and close cycle state when the pane is killed externally by tmux.
- [ ] Add a tmux `remain-on-exit` or pane option guard so the supervisor can detect pane death before tmux reaps the shell process.
- [ ] In tmux-router sync: before stashing or killing a pane, check if it is claimed by `agent-doc` (registry cross-check). If so, either:
  (a) send `agent-doc` an IPC shutdown request and wait for clean exit, or
  (b) skip stashing claimed panes entirely and let agent-doc manage their lifecycle, or
  (c) re-register the new pane to the same session and file after the swap.
- [ ] Add a crash-recovery heuristic in `agent-doc start` / `session_start`: if the previous session for the same file has no `session_end`, log a `session_crashed` event with the orphaned pane ID before starting the new session.
- [ ] Add regression test: simulate pane kill during active supervisor, verify session log records a crash event and cycle state is closed.
- [ ] Verify the fix from a live JetBrains → tmux editor navigation workflow.
