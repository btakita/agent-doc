---
name: reference_efs_repo_has_no_ci_workflow
description: "The equityfundingsource-dev repo has no GitHub Actions CI workflow — local `make check` + a verified live deploy are the only closeout gates"
metadata: 
  node_type: memory
  type: reference
  originSessionId: 69534568-363d-4c5b-9e80-26fa13a3f9b2
---

`src/equityfundingsource-dev` has **no** GitHub Actions CI workflow (`gh run list`
returns "could not find any workflows named CI" / empty). The standard agent-doc
closeout step "check the latest CI run" does not apply to EFS.

**How to apply:** for EFS turns, the closeout gates are (1) local `make check`
(`bun run test && bun run build && bun run build:worker` — 1600+ bun:tests; RTK
masks the exit code, so grep the run for the `N pass / M fail` summary, see
[[feedback_rtk_masks_make_check_exit]]) and (2) for deploy turns, verify the live
worker bundle carries your change via string-grep / content-hash match
([[reference_efs_verify_deploy_via_string_grep]]). Do not wait on or block for a
non-existent EFS CI run.

EFS tests are source-assertion style (`Bun.file(url).text()` + `toContain(...)`),
so editing an exact JSX/prop string in a component often breaks a test that pins
the old markup — update the pinned assertion in the same turn. Spec lives in
`prds/*.md`.
