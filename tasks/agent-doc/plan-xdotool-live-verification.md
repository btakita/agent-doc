# Plan: xdotool-driven live-verification harness (`#xdotool-lvbatch`)

Goal: automate the `#lvbatch` operator live-verification batch by injecting real
keystrokes into a live IntelliJ/Codex editor window with `xdotool`, so the agent
can drive **and** verify the IPC/CRDT live-load items without a human typing. This
turns the operator-gated items (`#exch-intermix-verify`,
`#postcommit-ipc-worktree-corruption`, `#saevon`, and the `#lvbatch` markers) into
agent-runnable verification.

## Environment facts (verified 2026-06-09, this machine)

- `DISPLAY=:0`, `WAYLAND_DISPLAY` empty, `XDG_SESSION_TYPE=tty` — X11 is reachable
  even though login is tty. `xdotool search`/`getwindowname` work.
- `xdotool` 4.20260303.1 at `/usr/bin/xdotool`. No `ydotool` (Wayland fallback) —
  if the desktop ever moves to native Wayland, xdotool stops working and this plan
  needs ydotool first.
- The live IntelliJ editor for a doc is targetable by title: window `41943270`
  was titled `agent-loop [~/work/btakita/agent-loop] – <abs doc path>`. Search with
  `xdotool search --name "<doc-basename>"`, then confirm the title before typing.
- tmux windows (the agent-doc supervisor panes) are also xdotool-addressable, and
  `tmux select-window`/`select-pane` can drive pane switches for `#tmux-switch-lag`.

## Safety guards (do these every run)

1. **Never type into the real working doc.** Drive a throwaway scratch repro doc
   under `.agent-doc/live-repro/xdotool-<case>.md`, opened in its own editor. The
   existing `.agent-doc/live-repro/*.md` files (e.g. `jb-cache-cancel.md`) are the
   pattern.
2. **Focus-guard before every `xdotool type`/`key`:** read `getactivewindow` +
   `getwindowname` and assert it equals the intended scratch editor; abort the run
   if not. A stray keystroke into the wrong window corrupts real work.
3. **Run from a FRESH session.** This degraded session reproduces
   `#postcommit-ipc-worktree-corruption` on every commit; verifying inside it masks
   real failures (especially `#saevon`'s false-success ack). The harness assumes a
   clean session started from the committed boundary.
4. **Timing by marker, not sleep.** Fire keystrokes when a sentinel appears in
   `.agent-doc/.../ops.log` / session-log (poll for the IPC-apply marker), not on a
   fixed `sleep`. Mid-finalize is the window that produces the drift class.

## Harness shape

`scripts/xdotool-live-verify.sh <case>` (or a small Rust SimWorld driver where the
assertion is deterministic):

1. Open the scratch doc in a dedicated IntelliJ editor; record its window id.
   Automated (`#activateinstalledjetbrai`): the harness asks the ALREADY-RUNNING IDE to
   open it (`$AGENT_DOC_IDE_LAUNCHER`, else `idea`) and polls for the window, so no human
   opens a file first. The open is refused for any path outside `.agent-doc/live-repro/`,
   and a cold IDE is still never launched.
2. Start an agent-doc cycle in the supervisor tmux pane (`finalize`/`write`).
3. Poll the log for the IPC-apply sentinel; when it appears, focus-guard, then
   `xdotool type --window <id>` a small edit (or `key` a few chars) into the doc to
   create the concurrent-edit timing.
4. Let the cycle finish; grep the log for the per-item marker (below) and assert.
5. Tear down the scratch doc; report pass/fail per item.

## Per-item verification recipes (markers to assert)

- **`#exch-intermix-verify`** — deliberate mid-finalize edit → expect
  `live_prompt_drift_auto_recovered` in ops.log; confirm `agent-doc commit` lands
  the response with no "manual cleanup" wedge. (We already see this marker firing
  live on other docs; this captures the deliberate agent-doc-bugs2 repro.)
- **`#postcommit-ipc-worktree-corruption`** — finalize a response that consumes a
  queue `do[#id]`; after closeout assert **working-tree == HEAD** (drift = the bug).
  Logs show `IPC reposition boundary signal sent` + `skipping working-tree boundary
  reposition — IPC listener active`. Add SimWorld coverage asserting tree==HEAD.
- **`#saevon`** — flip `EARLY_ACK_ENABLED=true` in `ipc_socket.rs`, `cargo build
  --release` + `agent-doc lib-install`, then drive typing load; expect
  `[ipc-socket] early-ack pending emitted before apply` with **no** false-success
  and **no** false ack-timeout.
- **`#lvbatch` markers** — `#4wxr` claim cross-session-reject (claim.rs +
  ClaimAction.kt); `#9adk` run/clear coalesced (route.rs + sync.rs); `#f5d2/#pcp6`
  ops.log `live_buffer_classify` + write.rs `visible_write_live_buffer_matches_disk`
  / `visible_write_deferred_current_changed`; `#saev` early-ack (only after
  `#saevon` flips the flag).
- **`#tmux-switch-lag`** — drive a doc-to-doc switch via tmux/editor action; the bug
  is a visible stash-frame flash, so assert programmatically on pane-move ordering
  (move-before-select) rather than pixels, OR capture frames (`xdotool`/`scrot`)
  during the switch. The code fix is the `sync.rs:700` move-before-select reorder +
  SimWorld ordering coverage; the harness only needs to confirm no intermediate
  stash frame.

## What this does NOT cover

- `#s760wire`'s transcript-token context-% reader is code to write in `route.rs`,
  not a typing test — xdotool does not unblock it.
- Items needing a JB plugin reinstall still need the install step; xdotool only
  drives input, it does not refresh the cdylib/plugin.

## Deliverable

A reusable harness script + deterministic SimWorld coverage for the assertions that
can be made offline (tree==HEAD after closeout; pane move-before-select ordering).
The xdotool path covers the genuinely-live timing the SimWorld can't exercise.
