---
name: Check working tree not just commit history
description: When assessing submodule release status, always check git status + git diff alongside git log
type: feedback
---

When checking if a submodule has pending changes (e.g., "any pending releases?"), always run BOTH:
1. `git log <tag>..HEAD` — committed but untagged changes
2. `git status --short` + `git diff --stat` — uncommitted working tree changes

**Why:** User caught me saying tmux-router had no pending release when it had uncommitted sync.rs changes. I only checked commit history and missed the working tree.

**How to apply:** Any time you assess whether a submodule is "up to date" or "needs a release", run both committed and uncommitted checks. Never say "no changes" based on `git log` alone.
