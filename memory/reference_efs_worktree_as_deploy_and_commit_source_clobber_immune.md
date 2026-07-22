---
name: reference_efs_worktree_as_deploy_and_commit_source_clobber_immune
description: "Isolated EFS deploy — port your edits into a clean HEAD worktree, then build/deploy/commit from IT; immune to a concurrent session clobbering the shared working tree."
metadata: 
  node_type: memory
  type: reference
  originSessionId: fb70f974-4e91-4b3a-80d7-07b7bb251da9
---

When a concurrent session is actively editing EFS files (proven when your edits to
a file silently revert mid-session), use a clean detached worktree as BOTH the deploy
source and the clean commit source. Ported changes there are immune to the shared
working tree getting clobbered.

Recipe (validated 2026-07-10, shipped per-type doc upload + unified lender combobox):
1. `HEAD_OLD=$(git rev-parse HEAD)`; `git worktree add --detach $WT HEAD`.
2. Symlink node_modules (root + `app/equityfundingsource/`) from the main checkout —
   do NOT `bun install` (fresh install drops Tailwind classes, e.g. md:grid-cols-3).
   Verify with `grep -c grid-cols-3 <built .css>`. See [[reference_efs_deploy_from_main_checkout_not_fresh_install_worktree]].
3. Port ONLY your changes into the worktree: `cp` files that were clean at HEAD
   baseline; for files that already carried other WIP, re-apply your exact edits to
   the worktree's pristine copy (your old_strings match HEAD). `git status` in the
   worktree must show exactly your files.
4. `cp $SRC/.env $WT/.env` (repo-root `.env` holds CLOUDFLARE_API_TOKEN/ACCOUNT_ID;
   cf-deploy reads repo_root/.env; worktree lacks it). wrangler.toml IS tracked so
   it's already present.
5. `bun run build:worker` then verify live-unique strings + CSS classes in the built
   `dist/worker-assets/*.js|*.css`.
6. Deploy frontend-only with `deploy:worker:no-migrate` (skips remote D1 migrate).
7. Commit cleanly from the worktree: `git add <your files>` (only your diff exists
   there) → `git commit` on detached HEAD → `NEW=$(git rev-parse HEAD)` →
   `git -C $SRC update-ref refs/heads/main $NEW $HEAD_OLD` (compare-and-swap; fails if
   main moved). Then in the main checkout `git reset --mixed HEAD -q` to realign the
   index — working tree + concurrent WIP untouched.
8. `rm -f $WT/.env` then `git worktree remove --force $WT`.

Result: prod + `main` HEAD both carry your change even if the concurrent session
overwrote your copy in the shared working tree (it did — it re-added the removed
field). Verify prod via string-grep of the served bundle ([[reference_efs_verify_deploy_via_string_grep]]).
Cleaner than the temp-index variant [[feedback_efs_commit_only_your_files_via_temp_index_compare_and_swap]]
when your changes share a file with someone else's WIP. Don't patch their WIP
([[feedback_concurrent_agent_doc_refactor_session_build_coordination]]). No push unless asked.
