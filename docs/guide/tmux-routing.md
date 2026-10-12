# Tmux Routing and Navigation Lanes

How editor hotkeys reach the right agent pane, how to spread documents across
several tmux sessions, and how two IDEs on one project can navigate tmux
independently. This page moved here from the project [README](../../README.md).

## The tmux-backed CLI cycle

```sh
# 1. Initialize project (creates .agent-doc/ and installs SKILL.md)
agent-doc init

# 2. Scaffold a session document
agent-doc init session.md "My Topic"

# 3. Claim the document to the current tmux pane
agent-doc claim session.md

# Or provision a distinct pane in this project's current tmux session
agent-doc claim another-session.md --new-pane

# Or name the document agent-doc.md and start it without a file argument
agent-doc init agent-doc.md "My Topic"
agent-doc start

# 4. Route hotkey triggers to the correct tmux pane
agent-doc route --dispatch-only --plain-trigger --wait-for-ready 60 session.md

# 5. Run: diff, send to agent, write response back
agent-doc run session.md
```

The typical edit cycle: write in your editor, trigger `agent-doc route --dispatch-only --plain-trigger --wait-for-ready 60 <file>` via a hotkey, and agent-doc dispatches the plain `agent-doc <file>` reopen into the right pane. Route observes Lazily current/typing state before it admits document work, and editor-triggered routing gives slow-starting supervisors a longer readiness window before surfacing a startup error. During that readiness window, route owns the pane input window, so supervisor queue drains and context-reset `/clear` / `/new` recovery wait instead of interleaving into the composer. That editor path always stays a bounded reopen send; it does not restart Codex just because the previous prompt was `/clear`.

## Remote workspaces

For Coder, JetBrains Gateway, and VS Code Remote SSH setup, including terminal
host precedence and headless failure behavior, see the
[Coder workspace terminal runbook](../../runbooks/coder-workspace.md).

## Several tmux sessions

To manage panes across several tmux sessions from one editor (a session per
topic or project), list them in `tmux_sessions` and bind documents with
`tmux_session:` frontmatter; see the
[multi-session tmux runbook](../../runbooks/multi-tmux-sessions.md).

## Per-IDE navigation lanes

When IntelliJ IDEA and Rider (or two installs of one IDE) are open on the same
project, they share one tmux `agent-doc` window by default, driven by whichever
IDE was focused last. To let each IDE navigate tmux on its own, opt in:

```toml
# .agent-doc/config.toml
tmux_session = "agent-loop"
editor_navigation = "per_installation"   # default: "shared"
```

Each installation then gets its own lane session `<tmux_session>-<installation>`
(for example `agent-loop-IU-0a1b2c3d`), created on that IDE's first layout sync
with `agent-doc` and `stash` windows. A document's pane stays in the lane that
started it: another IDE's layout heartbeats drop it (`cross_lane_layout_dropped` in
`ops.log`) and its focus is refused (`owned_by_other_lane`) instead of moving the
pane. To move it, run **Sync Tmux Layout** or **Claim for Tmux Pane** in the other
IDE: that explicit request hands the pane over to the requesting IDE's lane
(`navigation_lane_handoff`) and the IDE that lost it re-arranges without it. VS Code and older JetBrains plugins stay on the shared session. The mode is
read when the project controller starts; an unknown value logs a warning and
keeps `shared`. Each JetBrains IDE's agent-doc terminal attaches to its own lane
session, its manual Sync Tmux Layout arranges that lane, and tmux-to-editor focus
mirroring reads only that lane ([plan](../plan-ide-independent-nav.md)). A new
pane started from an IDE lands in that IDE's lane too: **Run Agent Doc** arranges
the requesting IDE's lane and starts a missing pane there, and the Claim actions
pass the IDE's client id (`agent-doc claim --client-id <id>`; `agent-doc route` and
`agent-doc start` accept the same flag). An existing pane is always used where it
lives; only an explicit Claim or Sync Tmux Layout moves it. **Run Agent Doc** on a
document whose pane lives in another IDE's lane runs it in that pane, leaves this
IDE's tmux layout as it was, and shows a notice naming the lane the pane is in
(`owned_by_other_lane`); use Claim for Tmux Pane to bring it over. When a supervisor
that a layout pass left out restarts, each IDE's lane that left it out re-applies
its own last layout in its own session; that re-arrangement never takes a pane
from another IDE's lane.

### Switching modes and cleanup

Switching the mode is safe on a project that already has a shared window.
Turning `per_installation` on moves no pane: existing panes stay in the shared
session, and each IDE's lane starts empty and gains panes from its own layouts
and explicit claims. Turning it back to `shared` and running `agent-doc gc`
moves every document pane in a lane session back into the shared session's
`stash` window (the same pane, nothing restarted), from where the shared layout
places it. Before you run `agent-doc gc`, the shared layout already works: a
document it shows whose pane is still in a lane session is brought into the shared
session (the same pane, never a second one), and the shared layout never arranges a
lane session, even when that pane or your terminal is still there. A document it
does not show waits in its lane until gc moves it. `agent-doc gc` also closes lane sessions that are empty: no document
pane, no running program, no attached terminal, and, after a switch back to
`shared`, nothing left to move. It only ever touches sessions agent-doc created
for this project (marked `@agent_doc_navigation_lane`), never a session that
just has a lane-like name. `agent-doc gc --dry-run` shows what it would do.

You rarely need to run it: the project controller does the same cleanup on its
own. When an IDE quits, its empty lane session closes; when the controller
starts under `shared`, it moves the lanes' panes back and closes the lanes
(`navigation_lane_reconcile_triggered` in `ops.log`). The controller only closes
the lane of an IDE it saw quit, so a lane left over from an IDE that was closed
while no controller ran waits for `agent-doc gc`.
