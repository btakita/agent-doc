---
name: reference_efs_deploy_from_main_checkout_not_fresh_install_worktree
description: "EFS deploys must build from the main checkout (or a node_modules-symlinked worktree), NOT a fresh-`bun install` worktree — fresh install breaks the CSS build. Plus concurrent-rebase recovery."
metadata: 
  node_type: memory
  type: reference
  originSessionId: b63621b5-c3b7-4683-9947-d063ed523dd8
---

Building an EFS (`src/equityfundingsource-dev`) worker for deploy from an **isolated worktree with a fresh `bun install`** produces a BROKEN CSS bundle: `bin/pack-assets.ts` fails with "no complete CSS bundle found" because the fresh-install build drops Tailwind utilities (e.g. `md:grid-cols-3`) from the worker CSS. Same dep versions (`@rebuildjs/tailwindcss@0.10.60`), no patches — the fresh install still differs from the original `node_modules`.

**Deploy from the main checkout** (its `node_modules` builds complete CSS — `css_href` set, `md:grid-cols-3` present), or a worktree that **symlinks** `node_modules` from the main checkout (but symlinked workspace pkgs `@btakita/ui--*` then resolve to the *original* `lib/`, so it can't include worktree-local `lib/` edits — land your commit on main and build there instead).

Portal base classes `.btn-o`/`.card`/`.topbar`/`.no-data` are **injected inline by `lib/ui--server--efs/portal.ts` SSR** (20+ `<style>`/class refs), NOT in the css bundle — their absence from the served worker CSS is NORMAL, not a regression. The css bundle only needs `md:grid-cols-3` + `.efs-container`/`.efs-section-py` markers.

`deploy:worker` (with migrate) applies committed migrations idempotently; safe when main has committed migrations the deployed code needs. Verify via served bundle string-grep (`curl / | grep index.browser-*.js`, fetch, grep unique literals) since admin API 401s. [[reference_efs_verify_deploy_via_string_grep]]

## Concurrent-session force-rebase recovery (2026-07-08)
A concurrent session `git pull --autostash`-ed and **force-rebased main, DROPPING my already-pushed commit** (HEAD moved 5f5ff92→e4342bc→a7f241b→(my 9972aac dropped)→e4dbf1e). Detect with `git merge-base --is-ancestor <mycommit> HEAD` (exit 1 = dropped). My work survived as an **orphan commit** (retrievable by hash) + a saved `git diff HEAD` patch. The autostash-reapply left `UU` conflict markers across *their* files (build-breaking).

Recovery when the other session is paused and you're sole session:
1. `git add -A` then commit the whole messy working tree (conflict markers, WIP, untracked) to a **backup branch** — preserves everything the other session had uncommitted.
2. `git checkout main && ` (main was clean-committed) — working tree resets clean.
3. `git cherry-pick <my-orphan-commit>` onto the new main tip (merged cleanly here even though they'd rewritten `loan_detail.tsx` +241 — 3-way merge worked). Fix any test-string assertions the behavior change touched.
4. Build + deploy from the main checkout. Verify markers live.
5. Leave their `stash@{0}` autostash + backup branch intact for them to resume. Related: [[feedback_concurrent_session_clobbers_efs_worker_deploy]] [[feedback_commit_to_head_to_stop_concurrent_deploy_clobber]] [[reference_efs_isolated_deploy_clean_worktree]]
