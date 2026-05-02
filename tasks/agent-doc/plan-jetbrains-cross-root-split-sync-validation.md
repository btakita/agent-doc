# Plan: JetBrains cross-root split sync validation

## Goal

Validate that a JetBrains editor split spanning the workspace root document and a nested submodule document now preserves the full markdown layout during passive sync and routed layout reporting.

## Steps

1. Reproduce the split with `tasks/agent-doc/agent-doc-bugs2.md` visible in one editor column and `src/boost-client/tasks/monsterrodholders.md` focused in the other.
2. Confirm JetBrains passive sync reports both visible markdown files as absolute `--col` layout entries instead of collapsing to a single-column `monsterrodholders.md` command.
3. Verify the tmux `agent-doc` window keeps two panes across the focus change, without stashing the workspace-root pane or triggering later stale-session cleanup for that document.
4. Capture any remaining `ops.log` / `/tmp/agent-doc-sync.log` drift if the layout still collapses, and reduce the surviving mismatch to a narrower follow-up bug.
