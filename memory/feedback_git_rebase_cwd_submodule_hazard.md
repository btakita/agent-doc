---
name: feedback_git_rebase_cwd_submodule_hazard
description: "Never run `git pull --rebase origin main` while cwd is a submodule — it rebases the submodule's branch onto the submodule's main, not the superproject."
metadata: 
  node_type: memory
  type: feedback
  originSessionId: f999e864-a341-42c3-9ea0-8df0ceecf300
---

When pushing fails (non-fast-forward) and you fall back to `git pull --rebase --autostash origin main`, the **cwd determines which repo gets rebased**. In agent-loop, Bash cwd persists across calls; after `cd src/agent-doc` for a submodule push, a follow-up `git pull --rebase origin main` rebases the **submodule's feature branch onto the submodule's `main`** (a destructive cross-branch rebase), leaving a detached HEAD with merge conflicts (e.g. `UU Makefile`) and files reverted to main's older state.

**Why:** the agent-loop superproject and `src/agent-doc` (etc.) are separate git repos. `origin/main` means something different in each. A rebase fallback meant for the superproject silently mangles the submodule when cwd is wrong.

**How to apply:**
- Before any `git pull --rebase`/`push` fallback, `cd /home/brian/work/btakita/agent-loop` (superproject root) explicitly — don't rely on persisted cwd from a prior submodule push.
- Push the submodule and the superproject as **separate, explicit** steps with explicit `cd`, not chained `||`/`if` fallbacks that inherit a stale cwd.
- Recovery if it happens: the local branch ref usually still matches origin (the rebase moved HEAD, not necessarily the branch ref). Verify `git rev-parse <branch>` == `git rev-parse origin/<branch>`, confirm all work is committed+pushed on the remote, then `git checkout -f <branch>` to abandon the rebase debris. Check `git stash list` is unchanged (don't pop pre-existing user stashes). The working-tree debris is safe to discard only after confirming the working tree was clean before the errant rebase.

**The same hazard bites `commit`/`push`, not just rebase (observed 2026-07-19, lazily `#lzspecedgeindex`).** While committing across six sibling submodules in one session, a `cd .../lazily-spec && make check` was followed by a bare `git add -A && git commit && git push` — which ran in **`lazily-dart`**, not `lazily-spec`. Result: dart's pre-existing uncommitted `AGENTS.md` (a tsift version-marker bump I had explicitly told agents to leave alone) was committed under a lazily-spec commit message and pushed to `lazily-hub/lazily-dart`. Bash cwd persistence across calls is **not** reliable enough to lean on when a prior tool result has said "Session cwd remains …".

**How to apply:** when working across multiple submodules in one session, always use **`git -C /abs/path/to/repo ...`** for `add`/`commit`/`push`/`status` rather than relying on a `cd` from an earlier call. Never pair `git add -A` with an inherited cwd. Recovery is clean if caught immediately: `git revert --no-edit <sha>`, then `git checkout <sha> -- <file>` + `git restore --staged <file>` to put the file back as an *uncommitted* modification, restoring the exact prior state, then push the revert. Prefer revert over force-push on a submodule that was already pushed. Related: [[feedback_efs_commit_only_your_files_via_temp_index_compare_and_swap]].

Related: [[feedback_recursive_submodule_commits]], [[feedback_verify_via_worktree_inside_src]].
