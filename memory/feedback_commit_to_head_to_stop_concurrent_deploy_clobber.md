---
name: feedback_commit_to_head_to_stop_concurrent_deploy_clobber
description: "When a concurrent session repeatedly reverts your uncommitted EFS deploy, commit to HEAD so their HEAD-based build includes your work"
metadata: 
  node_type: memory
  type: feedback
  originSessionId: 59642b8b-daef-411f-ab9e-5926a333fdf9
---

On the shared `equityfundingsource` worker, a concurrent session was deploying
from its own isolated worktree checked out at HEAD (the same clean-worktree
pattern in [[reference_efs_isolated_deploy_clean_worktree]]). Because my UI
changes were only **uncommitted** edits in the shared main tree, every one of
their HEAD-based builds excluded my work and reverted it live — I re-deployed,
they clobbered again (verified by grepping the live bundle: `pm-panel--full`
dropped from 2 back to 1). Deploying repeatedly does not converge.

**Why:** two sessions both deploy `deploy:worker` (replaces the worker wholesale,
per [[feedback_concurrent_session_clobbers_efs_worker_deploy]]); their build is
`HEAD`, mine was `HEAD + uncommitted`. Their build wins whenever it runs last.

**How to apply:** commit your changes into HEAD (scoped `git add` of only your
files — leave their uncommitted WIP in the working tree untouched), THEN build
+ deploy from a fresh isolated worktree at your new HEAD. Once your work is a
commit, the concurrent session's next HEAD-based worktree build includes it and
stops reverting it. Confirmed: after committing (`92878b0`, `f122761`) the
concurrent session stacked its next commit on top of mine and the live bundle
kept my changes. This overrides the usual "commit only when asked" default when
the user's goal is a durable live deploy under active concurrency. Residual
risk: a concurrent worktree pinned to a stale pre-your-commit SHA can still
clobber — that's their session's stale checkout, not fixable from yours.
