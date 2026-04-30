# 2026-04-24 / 2026-04-29 — submodule gitdir permission issue

- [ ] Reproduce the failure mode where work initiated from a submodule checkout needs to write through a gitdir under the parent repo's `.git/modules/...` tree, and confirm exactly which paths fall outside the session's writable roots.
- [ ] Lock in the Apr 29, 2026 nested-submodule repro from `src/boost-client/tasks/monsterrodholders.md`: `git commit` inside `src/boost-client/src/monsterrodholders-dev` failed with `fatal: Unable to create '/home/brian/work/btakita/agent-loop/.git/modules/src/boost-client/modules/src/monsterrodholders-dev/index.lock': Read-only file system`, which means the session could edit the working tree but not the real gitdir backing the nested submodule.
- [ ] Inspect `src/agent-doc` and the installed skill/runbook guidance to determine whether the fix belongs in binary-owned gitdir resolution, permission-aware preflight/commit guidance, or both.
- [ ] Implement the minimal fix so `agent-doc` handles submodule gitdirs under parent `.git/modules/...` paths correctly instead of treating the problem like a stale `index.lock` or an unexplained permission failure.
- [ ] Verify the resumed Codex launch path carries every required writable root for nested submodules, including the parent repo `.git`, the immediate submodule gitdir, and any deeper `.git/modules/...` gitdirs needed for commit/pointer updates.
- [ ] Add regression coverage and/or spec updates that lock in the intended behavior for parent-repo `.git/modules/...` gitdirs.
- [ ] Run the relevant verification in `src/agent-doc`, then patch the outcome back into `tasks/agent-doc/agent-doc-bugs.md` and clear or refine the pending item based on what remains.
