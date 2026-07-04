---
name: reference_agent_doc_build_worktree_needs_sibling_tmux_router
description: "agent-doc release builds need sibling ../tmux-router; clean build-worktrees must live under src/, not isolated /tmp"
metadata: 
  node_type: memory
  type: reference
  originSessionId: 162508b9-1570-4d1d-bf5a-13573ca3a864
---

`src/agent-doc/Cargo.toml` has `tmux-router = { path = "../tmux-router", version = "0.3.11" }`. A `git worktree` created to build a clean agent-doc commit (e.g. to install a fix WITHOUT a concurrent session's uncommitted WIP) MUST be placed as a sibling of `src/tmux-router` (for example `src/agent-doc-cleanfix`) so `../tmux-router` resolves. An isolated worktree under `/tmp/.../scratchpad` fails `make install-full` with `failed to read .../tmux-router/Cargo.toml (os error 2)`.

Also: RTK masks `make install-full` exit code as 0 even on failure — grep the log for `error[`/`make: ***`/`failed to`, don't trust the exit code (see [[feedback_rtk_masks_make_check_exit]]). Related concurrent-WIP coordination: [[feedback_concurrent_agent_doc_refactor_session_build_coordination]].
