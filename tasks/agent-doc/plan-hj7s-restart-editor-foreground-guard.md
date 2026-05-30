# Plan: #hj7s — refuse restart when an editor owns the pane TTY

## Bug

JB `Restart Supervisor Process` / `Interrupt and Restart` sends a raw `C-c`
interrupt (and then types the restart command) to the pane's **foreground**
process, not the supervisor. When a terminal editor owns the pane TTY — most
commonly Claude Code's `ctrl+g` "edit prompt in Nvim" — `C-c` is a no-op and the
follow-up restart-command keystrokes land in the editor buffer.

Live repro 2026-05-30: clicking `Interrupt and Restart` after a failed Restart
surfaced `vim`/`nvim` in the console; the raw `C-c` from
`session_actor_cmd.rs` `send_operator_interrupt_sequence` (and the routed
busy-interrupt in `route.rs`) went to the pane's foreground process.

## Decision (operator-confirmed)

The editor owning the pane is a **legitimate** operator state (`ctrl+g`
edit-in-nvim). Do **not** force-quit it (unlike the interrupt-clear path's
`:qa!`) and do **not** SIGINT around it. **Refuse** the restart with a clear
diagnostic and let the operator manually intervene (close the editor), following
UX conventions. `--force` must **not** bypass this guard.

## Fix

### Rust (`src/session_actor_cmd.rs`)

1. At the top of `prepare_restart_live_busy_pane`, build `live_pane_evidence`
   once and, before any force/non-force branch, check
   `terminal_editor_command(current_command)`. If an editor owns the pane:
   - log `session_restart_editor_holds_pane_refused` to ops_log, and
   - `anyhow::bail!` with `restart_editor_holds_pane_refusal_message(...)`.
2. New pure fn `restart_editor_holds_pane_refusal_message(file, pane, source,
   command, tail)` mirroring `restart_busy_refusal_message`'s field convention
   (`source=`, `current_command=`, `tail=`) with a distinctive header
   `session_restart refused for <file> because pane <pane> is held by editor
   <command>` plus guidance to close the editor (`:wq` in nvim) and retry.
3. Reuse the single `evidence` for the existing force branch (no double tmux
   query).

`--force` reaches the guard because it runs before the force branch — so both
`Restart` and `Interrupt and Restart` refuse cleanly when an editor holds the
pane.

### JetBrains plugin (`TerminalUtil.kt`)

4. `EDITOR_RESTART_REFUSAL_HEADER_REGEX` matching the new header; data class
   `EditorHoldsPaneRestartRefusal`; `parseEditorHoldsPaneRestartRefusal` mirroring
   `parseBusySessionRestartRefusal`; `notifyEditorHoldsPaneRestartBlocked`
   mirroring `notifyBusySessionRestartBlocked`.
5. Wire into `runRestartSupervisorCommand` `onFailure` for **both** force and
   non-force (the force path currently has `onFailure = null`, so an
   Interrupt-and-Restart refusal is otherwise silent). Check the editor refusal
   first, then busy, then starting, then generic.
6. Bump `pluginVersion` (per-edit) and build the signed zip.

## Tests

- Rust unit: `restart_editor_holds_pane_refusal_message` contains the parseable
  header, `current_command=`, the editor name, and the close-editor guidance.
- Rust unit: `terminal_editor_command` already covers the vim family.
- (Guard wiring is exercised through the pure message fn; the live tmux send
  path stays `#[ignore]`/`make tmux-ci` per project convention.)

## Scope / gate

`route.rs` routed busy-interrupt (`route.rs` ~6427) shares the raw-`C-c`-into-
foreground shape but is a distinct reroute path where a hard refusal would change
reroute semantics; tracked as a follow-up rather than forced into this cycle.

JB notification is **live-verify gated**: install the signed plugin, open
`ctrl+g` nvim in a Claude Code pane, click `Interrupt and Restart`, confirm the
refusal notification and that no keys reach the editor buffer.

## Likely files

- `src/session_actor_cmd.rs`
- `editors/jetbrains/src/main/kotlin/com/github/btakita/agentdoc/TerminalUtil.kt`
- `editors/jetbrains/gradle.properties`
