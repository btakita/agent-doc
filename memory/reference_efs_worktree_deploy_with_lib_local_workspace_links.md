---
name: reference_efs_worktree_deploy_with_lib_local_workspace_links
description: "EFS clean-checkout deploy CAN include worktree-local lib/ edits — mirror node_modules entry-by-entry and rebuild the @btakita workspace symlinks relative to the worktree, at app/ AND every lib/*/node_modules."
metadata:
  node_type: memory
  type: reference
---

Validated 2026-07-30 deploying EFS `29f27773` to `equityfundingsource-production`
while a concurrent session held ~355 uncommitted lines (including
`lib/ui--browser--efs/portal/admin/tmo_contacts.tsx` / `tmo_los.tsx`).

[[reference_efs_deploy_from_main_checkout_not_fresh_install_worktree]] says a
node_modules-symlinked worktree "can't include worktree-local `lib/` edits — land
your commit on main and build there instead." **That limitation is avoidable**, and
avoiding it is what makes a worktree deploy actually isolated: symlinking
`node_modules` wholesale makes `@btakita/ui--*` resolve through the symlink's
*physical* target, i.e. the MAIN checkout's `lib/` — so the concurrent session's
uncommitted `lib/` edits ship to production.

Recipe (do NOT `bun install` — that still breaks the Tailwind CSS bundle):

1. `git worktree add --detach $WT <sha>`
2. Root `node_modules`: plain symlink to `$SRC/node_modules` is safe — it contains
   no `lib/` references (verify: `ls -d node_modules/@btakita` must not exist).
3. `$WT/app/equityfundingsource/node_modules/`: real directory, symlink every entry
   (including dotfiles `.bin .bun .cache .mf`) to `$SRC`'s, **except `@btakita`**.
   Recreate `@btakita/` as a real dir with RELATIVE links
   `ln -s ../../../../lib/<pkg>` so they resolve inside `$WT`.
4. Repeat for **every `lib/*/node_modules`** — `lib/ui--server--efs/node_modules/@btakita/domain--server--efs -> ../../../domain--server--efs`.
   Missing this step is not a silent wrong-source build, it is a hard
   `Cannot find module '@btakita/domain--server--efs/deals'` from
   `lib/ui--server--efs/home/index.ts` (7 test failures). Fixing it took the suite
   from 2027 pass/7 fail to 2115 pass/0 fail.
5. `cp $SRC/.env $WT/.env`; `git -C $WT status --short` must show only `node_modules`.
6. `bun test`, then `bun run deploy:worker:production` (or `:no-migrate`).
7. `rm -f $WT/.env && git worktree remove --force $WT`.

**Isolation proof = content-hashed asset names.** The live page's
`index.browser-<HASH>.js` / `index.server-<HASH>.css` must equal the clean local
build's. Identical hash ⇒ identical source ⇒ the concurrent WIP did not ship. This
is stronger than string-grep ([[reference_efs_verify_deploy_via_string_grep]]) when
your change adds no new string literal (a pure control-flow gate).

Related: [[reference_efs_worktree_as_deploy_and_commit_source_clobber_immune]],
[[feedback_concurrent_agent_doc_refactor_session_build_coordination]].
