---
name: feedback_agent_doc_isolated_worktree_supervisor_critical
description: How to do supervisor-critical agent-doc submodule work in an isolated worktree without disrupting the live route-owned supervisor
metadata: 
  node_type: memory
  type: feedback
  originSessionId: ddfac8fe-17f4-434d-a871-7ec86fe70741
---

When a plan gates supervisor-in-process-critical agent-doc work to a "fresh, non-victim session" (e.g. `#xdocsuper`, `#queuestatemachine`, `#mergestatemachine`) but the operator wants it done NOW from the live session, do it in an **isolated git worktree of the agent-doc submodule** dispatched to a subagent.

**Why:** **How to apply:**
- The `agent-doc` workspace has an external sibling path dep `tmux-router = { path = "../tmux-router" }` (and `../../tmux-router` from the orchestration crate) → both resolve to `src/tmux-router`. So the worktree MUST be placed as a sibling of `tmux-router` under `src/`, or the build breaks. Use: `cd src/agent-doc && git worktree add -b <branch> ../agent-doc-<x>-wt HEAD`. That puts it at `src/agent-doc-<x>-wt` where `../tmux-router` still resolves. `agent-kit` is a crates.io dep (no path), no patch section.
- The live route-owned supervisor is only disrupted by **install/recycle**, NOT by in-place editing/building. So the subagent runs `make check` (debug nextest, never touches the installed/release binary) + commits on the branch, but does **NOT** `cargo install`/`make install`/`admin recycle`/push. Code lands on a branch; merge+install+live-verify is deferred to the operator (gate the backlog item `[/]`/operator-verify).
- Recording the result in the session doc still hits the recurring degraded-supervisor drift (`live_prompt_drift_after_preflight` + `postcommit_worktree_check match=false`): finalize commits the correct response to HEAD via `content_ours`, but the working tree gets mangled and the granular `--pending-gate` edit may not land in HEAD's review block. Recover per [[feedback_post_commit_worktree_drift_checkout_head_recovery]]: verify `git show HEAD:FILE` is correct, `git checkout HEAD -- FILE`, `agent-doc reset --from-current`, confirm `session-check` says committed. Don't chase a perfectly-consistent gate line — the prose response + committed branch are what matter.

Proven 2026-06-25: `#xdocsuper0` (claim consults `supervisor_lease_is_fresh_or_alive` before `AcceptStale`) shipped this way — worktree `src/agent-doc-xdocsuper0-wt`, branch `xdocsuper0`, commit `2f704263`, make check 5152 passed. See [[feedback_deep_supervisor_refactor_fresh_focused_cycle]].
