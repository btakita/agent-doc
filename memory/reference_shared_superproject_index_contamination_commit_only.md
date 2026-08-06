---
name: reference_shared_superproject_index_contamination_commit_only
description: "In the agent-loop superproject, other live panes leave files PRE-STAGED in the shared index, so `git add <mine> && git commit` silently commits their work — use `git commit --only -- <paths>`."
metadata: 
  node_type: memory
  type: reference
  originSessionId: 9dd22c19-7622-43b4-bf89-4cd9f14119a3
  modified: 2026-07-30T03:45:47.247Z
---

The `agent-loop` superproject index is shared by every concurrent pane. Another session can
leave files staged there at any moment. So this sequence commits **their** work under your
message:

```bash
git add -- mine.md          # index now = mine.md + whatever they staged
git commit -m "..."         # commits all of it
```

Observed 2026-07-29: `git add tasks/professional/equityfundingsource{,.done}.md` produced a
4-file staged set — `.gitmodules` and `src/haiven-dev` were already staged by another pane
(mid-submodule-add). The commit swallowed both.

**Always commit superproject paths explicitly:**

```bash
git commit --only -F - -- <path> [<path>...] <<'MSG'
...
MSG
```

`--only` commits HEAD plus exactly the named paths and leaves the rest of the index intact, so
the other session's staged state survives untouched. Verify with `git diff --cached --name-only`
after committing — their files should still be listed.

**Recovery if you already over-committed** (before pushing): `git reset --soft HEAD~1` restores
the *full* pre-commit index (yours + theirs), then re-commit with `--only`. Do not
`reset --hard` and do not `git restore --staged`.

Same discipline as [[feedback_efs_commit_only_your_files_via_temp_index_compare_and_swap]], but
the superproject needs it more often because every pane's agent-doc cycle touches it. See also
[[feedback_concurrent_sessions_force_push_shared_superproject]] and
[[feedback_never_dispatch_concurrent_agents_into_one_repo]].
