# Plan — restore Codex `Ctrl+D` supervisor quit menu

**Pending:** `#ctrldmenu` in `tasks/agent-doc/agent-doc-bugs2.md`
**Status:** Implemented

## Problem

Pressing `Ctrl+D` in a tmux-backed Codex `agent-doc` session currently exits the child and immediately restarts the supervisor path instead of presenting the cooked-mode quit menu with `q`.

Current evidence from local source state on 2026-04-28:

- `src/agent-doc/src/start.rs` now routes `restart_continue_exit_strategy(false, true, 0)` to `RestartFresh`
- the clean-exit branch logs `ctrl_d_restart_fresh` and prints `Restarting fresh to keep the pane attached...`
- the older prompt path (`Press Enter to restart, or 'q' to exit.`) is no longer used for the Codex `Ctrl+D` case
- `src/agent-doc/specs/supervisor.md` was edited to describe this same restart-fresh behavior, so code and spec currently agree on the wrong contract

This regresses the expected user-facing behavior: `Ctrl+D` should let the user choose whether to quit the supervisor entirely or restart, rather than forcing an automatic restart.

## Goal

Restore a deterministic quit-menu flow for Codex `Ctrl+D` / stdin EOF that preserves the existing failed-resume safeguards but does not bypass the user's chance to press `q`.

## Desired Behavior

- when Codex exits because the stdin->pty path forwarded `Ctrl+D` / EOF, supervisor switches back to cooked mode and shows the quit menu
- pressing `q` exits the supervisor cleanly
- pressing Enter restarts the session in the intended mode
- failed resume handoff protection remains intact for real resume failures and is not conflated with deliberate `Ctrl+D`
- docs/spec/tests all describe the same contract

## Implementation Steps

- [ ] Trace the current Codex clean-exit decision path in `src/agent-doc/src/start.rs`, especially `restart_continue_exit_strategy(...)` and the `RestartAction::PromptUser` handling
- [ ] Define the intended `Ctrl+D` contract explicitly:
  - whether it should reuse the generic clean-exit prompt text or a Codex-specific variant
  - whether Enter should restart fresh or resume after an intentional `Ctrl+D`
- [ ] Update the strategy branch so `Ctrl+D` reaches a prompt path instead of unconditional `RestartFresh`
- [ ] Restore or tighten the supervisor log events for this branch so the cause is still visible in logs even though the user sees a menu
- [ ] Update `src/agent-doc/specs/supervisor.md` to match the corrected behavior
- [ ] Add regressions for:
  - default clean Codex exit still resumes when `Ctrl+D` was not involved
  - `Ctrl+D` / stdin EOF selects the prompt path
  - repeated failed resume handoffs still escalate to prompt/fail-closed behavior independently of `Ctrl+D`
- [ ] Kill the live supervisor process during repro/verification and capture the exact teardown/restart behavior in logs so the manual test distinguishes a deliberate supervisor death from the `Ctrl+D` child-exit path
- [ ] Verify in a real tmux-backed Codex session that pressing `Ctrl+D` presents the menu and accepts `q`

## Files Likely To Change

- `src/agent-doc/src/start.rs`
- `src/agent-doc/specs/supervisor.md`
- `src/agent-doc/VERSIONS.md`

## Exit Criteria

- pressing `Ctrl+D` no longer auto-restarts before the user can quit
- the supervisor log still records that the clean exit came from stdin EOF / `Ctrl+D`
- code, spec, and regression tests agree on the same quit-menu behavior
