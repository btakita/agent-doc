---
name: feedback-never-dispatch-concurrent-agents-into-one-repo
description: "Never run two subagents against the same repo at once — one will git reset/stash the other's uncommitted work mid-task"
metadata: 
  node_type: memory
  type: feedback
  originSessionId: 5c0004fa-7f81-4b38-93d3-42eeb0b59a44
---

**Serialize subagents per repository.** Two agents working the same git repo
concurrently will destroy each other's uncommitted work, because isolating a
test run means `git stash` / `git reset`, and neither agent knows the dirty
files belong to someone else.

Observed 2026-07-19 (`#lzspecedgeindex`): I dispatched a cross-binding
fixture-drift agent and a `lazily-zig` double-free agent at the same time, and
both touched `lazily-zig`. The fixture agent ran `git reset` (reflog showed two
`reset: moving to HEAD`) to isolate its own test run, **wiping the other
agent's in-flight fix**. It restored a stash afterwards, but what came back was
a *partial* snapshot — an early first-pass fix missing a whole field and one of
the two fix sites. The second agent had to redo the work in an isolated
worktree. It also misread the other's deliberately-failing proving-test as
"their work is red", and reported the repo as broken when it wasn't.

I compounded it by inspecting `git status`, seeing the three files still
modified, and telling the user the tree was intact. It wasn't — the *contents*
had been rolled back. **A clean-looking `git status` does not prove uncommitted
work survived; only the owning agent can confirm that.**

**How to apply:**
- Before dispatching, list which repos each agent will touch and check for
  overlap — including indirect ones (a "cross-binding" task touches every
  binding). Serialize any that collide.
- If concurrency is genuinely needed in one repo, require `isolation:
  "worktree"` so each agent gets its own checkout.
- Tell agents whose repo may be shared: never `git reset`/`git stash` files you
  did not modify; use a worktree to isolate instead.
- When an agent reports another session's tree as broken, suspect a
  proving-test or in-flight state before believing it — and ask the owner.

Related: [[feedback_git_rebase_cwd_submodule_hazard]] (the `git -C` lesson from
the same session), [[feedback_concurrent_sessions_force_push_shared_superproject]].
