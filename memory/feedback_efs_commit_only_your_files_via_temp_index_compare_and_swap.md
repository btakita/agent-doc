---
name: feedback_efs_commit_only_your_files_via_temp_index_compare_and_swap
description: "Commit only your files onto a live-moving shared EFS HEAD (foreign WIP + wedged index present) via a temp-index + update-ref compare-and-swap"
metadata:
  node_type: memory
  type: feedback
---

On the shared `equityfundingsource` checkout a concurrent session often leaves
the tree un-committable for a normal `git commit`: unmerged index entries from
an abandoned `git stash pop` (conflict markers `<<<<<<< Updated upstream` /
`>>>>>>> Stashed changes`), plus their own uncommitted WIP, plus **your** edit
sitting in files they also touched. You still need only-your-files committed to
HEAD so their next HEAD-based build carries your change (see
[[feedback_commit_to_head_to_stop_concurrent_deploy_clobber]]).

**Technique — commit exact paths without touching their index or WIP:**
```
BASE=$(git rev-parse HEAD)
TMPIDX=$(mktemp -u)
GIT_INDEX_FILE=$TMPIDX git read-tree "$BASE"          # clean base index, no unmerged entries
GIT_INDEX_FILE=$TMPIDX git add -- <your files>        # stage working-tree copies
TREE=$(GIT_INDEX_FILE=$TMPIDX git write-tree)
COMMIT=$(git commit-tree "$TREE" -p "$BASE" -m "...")
git update-ref refs/heads/main "$COMMIT" "$BASE"      # compare-and-swap: fails if they committed
rm -f "$TMPIDX"
```
`update-ref <new> <old>` is the safety net: if the concurrent session committed
in the meantime it refuses ("is at X but expected Y") instead of clobbering —
re-read HEAD, rebuild your edit on the new base, retry. Verified 2026-07-08: it
caught their `feat(admin-lender-preview)` commit landing mid-flight.

**Pitfalls learned the hard way (2026-07-08, Wire-tab + Additional-Documents task):**
- The file you edited may ALREADY carry foreign WIP before you touch it (check
  the *pre-edit* `git diff HEAD` line count vs your intended edit size). If the
  diffstat is far bigger than your change, you bundled their WIP. Fix per
  [[reference_efs_isolated_deploy_clean_worktree]] "shared file" recipe:
  `git show <base>:<file> > <file>`, re-apply ONLY your edits, rebuild the commit.
- `update-ref` does NOT sync the working index → `git status` then shows `MM`/`D `
  artifacts. Clean with `git reset --mixed HEAD` (index→HEAD, working tree
  untouched) so the shared status isn't confusing.
- To ship ONLY your commit when foreign server files are uncommitted: `tar` them,
  `git show HEAD:<f> > <f>` to swap to committed content, `deploy:worker`, then
  `tar -x` to restore; verify byte-identical with `tar -df`. Deploy once, verify
  live string markers ([[reference_efs_verify_deploy_via_string_grep]]), STOP —
  don't fight a live session.
