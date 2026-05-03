# Plan: monsterrodholders duplicate column-memory replay

## Goal

Validate the mixed-split `monsterrodholders.md` passive sync flow after the duplicate column-memory guard change, with focus on preventing stale `last_layout.json` state from collapsing the `agent-doc` tmux window to one pane or restoring the wrong document.

## Steps

1. Reproduce the JetBrains mixed-split layout with `src/boost-client/tasks/monsterrodholders.md` focused on the right and a non-markdown sibling on the left.
2. Inspect `src/boost-client/.agent-doc/last_layout.json` before and after passive sync to confirm the state no longer settles into duplicated `monsterrodholders.md` entries.
3. Confirm the `agent-doc` tmux window shows two panes with `monsterrodholders.md` on the right and in focus, without reviving `tasks/agent-doc/agent-doc-bugs2.md`.
4. Capture any remaining drift in `ops.log` / `/tmp/agent-doc-sync.log` and either close the issue or turn the surviving mismatch into a narrower follow-up bug.
