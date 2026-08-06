---
name: feedback_git_commit_only_silently_undoes_rm_cached
description: "`git commit --only <paths>` rebuilds those paths from the WORKING TREE, so it silently reverts a `git rm --cached` and leaves the file tracked — and `git status` then looks clean, exactly like success."
metadata: 
  node_type: memory
  type: feedback
  originSessionId: e02d13b6-2abb-4ec9-baeb-f98355152c7a
  modified: 2026-08-03T03:06:24.675Z
---

Hit 2026-08-02 untracking a committed credential in `censible-core`.

`git rm --cached apps/*/.env` stages the deletions but leaves the files on disk
(that is the point — direnv still needs them locally). Committing that with the
house-standard `git commit --only -- <paths>` — used everywhere here to avoid
[[reference_shared_superproject_index_contamination_commit_only]] — **undid the
untracking**: `--only` builds the commit from those paths' *working-tree* state,
and since the files still existed, it put them straight back in the index.

The commit succeeded, reported `2 files changed` instead of 5, and `git status`
came back clean. Nothing looked wrong. The credential was still tracked, and I
had already started reporting the work as done.

**Why:** `--only` means "commit exactly these paths as they are in the working
tree", which is the opposite of "commit the index entries I just staged for these
paths". For a staged *deletion* of a file that still exists on disk, those two
readings disagree.

**How to apply:** when the change is a `git rm --cached` (index-only deletion of a
file that stays on disk), do NOT use `--only`. Stage it, verify the index with
`git diff --cached --name-status` **and** confirm `git diff --name-only` is empty
so nothing else rides along, then commit the index directly with a plain
`git commit`. Verify from the result, not the exit status:

```
git ls-files | grep -E '<path>'      # must be empty
git ls-tree -r --name-only HEAD      # must not list it
```

The general lesson: after any untracking commit, prove it from `git ls-files` /
`git ls-tree HEAD`. A clean `git status` is not evidence — it is equally
consistent with the untracking having been silently reverted. Related:
[[feedback_check_working_tree]], [[feedback_no_hallucinate_correct_errors]].
