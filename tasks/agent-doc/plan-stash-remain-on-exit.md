# Plan — remain-on-exit must follow pane to stash window

**Pending:** `#stshroe`
**Status:** Draft

## Problem

When a pane is stashed (moved from the agent-doc window to a stash window via `join-pane`), the `remain-on-exit` setting does not follow it. The setting is applied at the **window level** (`set-option -t <window_id> remain-on-exit on`), but tmux window options stay on the window, not the pane. If the Claude/Codex process exits while in the stash window (which lacks `remain-on-exit`), the pane is destroyed immediately. When the user switches back, sync finds "no alive pane" and auto-starts a replacement, losing the previous session context.

**Repro (2026-04-30):**
- `tasks/claudescore-3.md` pane `%530` in window `@104` (has `remain-on-exit on`)
- User switches to another file → sync stashes `%530` to a stash window
- Claude process exits silently while stashed → pane destroyed (no `remain-on-exit` in stash window)
- User switches back → sync detects `%530` dead, auto-starts `%538`
- Session log shows `session_superseded old_pane=%530 new_pane=%538` with no `claude_exit` event

**Evidence:**
- `tmux show-options -p -t %538 remain-on-exit` → (empty, no pane-level setting)
- `tmux show-options -w -t @104 remain-on-exit` → `on` (window level only)

## Fix

1. **Change `enable_remain_on_exit` to use pane-level option.** In `src/tmux-router/src/tmux.rs`, change from `set-option -t <window_id>` to `set-option -p -t <pane_id> remain-on-exit on`. This makes the setting follow the pane across window moves (stash/rescue).

2. **Also set `remain-on-exit` on the stash window itself.** In `ensure_stash_window` or `stash_pane`, add `set-option -w -t <stash_window> remain-on-exit on` as belt-and-suspenders protection for panes that lack the pane-level setting (e.g., manually created panes).

3. **Add regression coverage:** test that a pane moved from a `remain-on-exit on` window to a stash window still retains the dead pane after process exit.

4. **Investigate why Claude exited while stashed.** The `remain-on-exit` fix preserves the pane for recovery, but the Claude process should not be exiting while stashed in the first place. Check whether tmux sends SIGHUP during `join-pane` or if there is an idle timeout.
