# 2026-04-24 — `agent-doc-bugs` submodule gitdir permission issue

- [ ] Reproduce the failure mode where work initiated from a submodule checkout needs to write through a gitdir under the parent repo's `.git/modules/...` tree, and confirm exactly which paths fall outside the session's writable roots.
- [ ] Inspect `src/agent-doc` and the installed skill/runbook guidance to determine whether the fix belongs in binary-owned gitdir resolution, permission-aware preflight/commit guidance, or both.
- [ ] Implement the minimal fix so `agent-doc` handles submodule gitdirs under parent `.git/modules/...` paths correctly instead of treating the problem like a stale `index.lock` or an unexplained permission failure.
- [ ] Add regression coverage and/or spec updates that lock in the intended behavior for parent-repo `.git/modules/...` gitdirs.
- [ ] Run the relevant verification in `src/agent-doc`, then patch the outcome back into `tasks/agent-doc/agent-doc-bugs.md` and clear or refine the pending item based on what remains.
