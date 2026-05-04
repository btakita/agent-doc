# 2026-04-24 / 2026-04-29 — submodule gitdir permission issue

- [ ] Reproduce the failure mode where work initiated from a submodule checkout needs to write through a gitdir under the parent repo's `.git/modules/...` tree, and confirm exactly which paths fall outside the session's writable roots.
- [ ] Lock in the Apr 29, 2026 nested-submodule repro from `src/boost-client/tasks/monsterrodholders.md`: `git commit` inside `src/boost-client/src/monsterrodholders-dev` failed with `fatal: Unable to create '/home/brian/work/btakita/agent-loop/.git/modules/src/boost-client/modules/src/monsterrodholders-dev/index.lock': Read-only file system`, which means the session could edit the working tree but not the real gitdir backing the nested submodule.
- [ ] Inspect `src/agent-doc` and the installed skill/runbook guidance to determine whether the fix belongs in binary-owned gitdir resolution, permission-aware preflight/commit guidance, or both.
- [ ] Implement the minimal fix so `agent-doc` handles submodule gitdirs under parent `.git/modules/...` paths correctly instead of treating the problem like a stale `index.lock` or an unexplained permission failure.
- [ ] Verify the resumed Codex launch path carries every required writable root for nested submodules, including the parent repo `.git`, the immediate submodule gitdir, and any deeper `.git/modules/...` gitdirs needed for commit/pointer updates.
- [ ] Add regression coverage and/or spec updates that lock in the intended behavior for parent-repo `.git/modules/...` gitdirs.
- [ ] Run the relevant verification in `src/agent-doc`, then patch the outcome back into `tasks/agent-doc/agent-doc-bugs.md` and clear or refine the pending item based on what remains.

## 2026-05-03 investigation note

- The source tree in `src/agent-doc` already contains the intended fix for this bug:
  - `fad7d54` — `fix: add submodule gitdir access for workspace sessions`
  - `f6e66ff` — `fix: expose nested submodule gitdirs`
  - `ffdc1b6` — `fix Codex submodule handoff` (`codex exec resume` strips `--add-dir` and relies on inherited writable roots)
- Current `HEAD` in `src/agent-doc` is `2153b02` on `20260422/codex-agent-backend`, which contains `f6e66ff`.
- The active installed binary is `agent-doc 0.33.16`.
- Targeted regression verification passed locally on 2026-05-03:
  - `cargo test append_workspace_access_args_adds_nested_submodule_gitdirs --manifest-path src/agent-doc/Cargo.toml`
  - `cargo test workspace_access_dirs_for_submodule_doc_includes_superproject_root --manifest-path src/agent-doc/Cargo.toml`
- The historical `agent-doc` capture for this exact bug already records the fix path: `.agent-doc/captures/483039e7a9b103bd5b68223dac05a4c449ac03899aba53d6022e82100578b5b0/cycle-1777494216335.json`.
- Current local evidence does **not** show a fresh regression in nested submodule writable-root calculation. Instead:
  - `tmp/agent-doc-sync.log` shows repeated pane/session churn for `src/boost-client/tasks/monsterrodholders.md`, including many pane recoveries and auto-starts.
  - current `ops.log` matches recent root-repo `.git/index.lock` failures, but I did not find a fresh `.../.git/modules/src/boost-client/modules/src/monsterrodholders-dev/index.lock` failure in current `agent-doc` logs.
- Working diagnosis:
  - if the user-facing failure is from the Apr 29 repro, the source-side bug is already fixed;
  - if a current Codex pane still hits the nested submodule lock path, it is more likely running on a stale pre-fix session / inherited permission envelope, or the failure is being conflated with the separate root `.git/index.lock` write problem.
- Next proof needed before reopening the code bug:
  - capture a fresh `codex_start mode=fresh` session for `src/boost-client/tasks/monsterrodholders.md` and record the exact failing `index.lock` path from that fresh pane.
