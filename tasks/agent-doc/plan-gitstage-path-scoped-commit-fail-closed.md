# Plan: `#gitstage` path-scoped commit fail-closed

## Bug

In the earlier `src/boost-client/tasks/monsterrodholders.md` closeout family, the intended root-repo follow-up was a narrow submodule-pointer bump in the superproject. The stage step for that narrowed path failed (`git add src/monsterrodholders-dev` hit `.../.git/modules/src/boost-client/index.lock: File exists`), but a later root `git commit` still succeeded because unrelated paths were already staged in the index.

That means the current session-doc safety rule is still incomplete:

- the skill/runbook guidance now tells the agent to keep the active session document out of manual repo commits;
- but it does **not** yet require proof that a narrowed manual repo commit is committing the intended staged set and nothing else.

The failure mode is: one stage command fails, the repo index is already dirty, and a later bare `git commit` consumes whatever happened to be staged instead of fail-closing.

## Evidence

- Backlog item `#gitstage` in `tasks/agent-doc/agent-doc-bugs2.md` records the exact user-facing regression: failed `git add src/monsterrodholders-dev`, later successful root commit, intended pointer left unstaged, unrelated local commit produced instead.
- Current workspace state is still the kind of environment that makes this dangerous: `git status -sb` at the root currently shows `main...origin/main [ahead 1]` plus many unrelated dirty paths across the superproject and several submodules.
- The installed/bundled skill guidance already covers only one part of the safety contract:
  - `src/agent-doc/SKILL.md`
  - `src/agent-doc/runbooks/commit.md`
  - `src/agent-doc/runbooks/harness-invocation.md`
  - `src/agent-doc/runbooks/compound-task-steering.md`
  These now say the active session document must stay out of ordinary repo commits, but they do not yet say that a narrowed repo commit must verify the staged set against the requested path set before committing.
- In the binary, `src/agent-doc/src/git.rs` already has one safe narrow-path example:
  - `update_parent_submodule_pointer(...)` uses `git commit ... -- <rel_path>` after a matching `git add -- <rel_path>`.
  - `git::commit()` also owns its own session-doc staging path.
  - The remaining hole is the manual repo-work flow executed by the harness/skill when a document directive says `commit + push`.

## Reproduction Target

Build a deterministic repro that does **not** depend on the live `boost-client` repo:

1. Create a fixture git repo with:
   - one intended path that should be committed;
   - one unrelated path already staged in the index.
2. Simulate a narrowed stage failure for the intended path:
   - real `index.lock` contention if practical in a test fixture; otherwise a controlled stage failure that leaves the unrelated staged entry intact.
3. Run the same style of follow-up commit the skill/harness would run after `commit + push`.
4. Prove the bad outcome:
   - the commit succeeds;
   - the unrelated path lands in history;
   - the intended path is still unstaged or uncommitted.

## Root-Cause Questions

1. Where is the unsafe assumption today?
   - skill/runbook instructions?
   - any binary-owned helper surface for repo commits?
   - both?
2. What exact commit forms are still allowed after a partial or failed stage?
   - bare `git commit -m ...`
   - pathspec commit with the wrong path list
   - commit after a failed `git add` without re-checking the index
3. What should count as proof that the intended staged set is correct?
   - exact path equality?
   - pathset superset/subset rules?
   - explicit handling for submodule pointers (`160000` entries)?
4. Where should the fail-closed rule live so it helps both Claude and Codex skill installs?
   - bundled `SKILL.md` / runbooks only
   - a binary helper or validator
   - both

## Proposed Fix Shape

### Layer 1: instruction-level fail-closed rule

Update the bundled skill/runbooks so a manual repo `commit + push` in an `agent-doc` turn must follow this contract:

1. Resolve the exact intended non-session path set first.
2. Run all stage commands for that path set.
3. If any `git add` / stage command fails, stop immediately. Do **not** proceed to `git commit`.
4. Before committing, compare the intended path set with `git diff --cached --name-only` (or a stricter staged diff inspection when submodule pointers are involved).
5. Only commit when the staged set matches the intended set closely enough for that workflow.
6. Use an explicit pathspec commit or equivalent isolated-index strategy so unrelated staged entries cannot leak into the commit.

### Layer 2: binary/test-backed guard

Investigate whether `agent-doc` should provide a binary-owned helper or validation surface for path-scoped repo commits, instead of leaving this entirely to prose discipline. Candidate shapes:

- a small helper command that stages + validates + commits a requested path set safely;
- a reusable internal validator used by future binary-owned repo-work surfaces;
- at minimum, regression tests in `src/agent-doc/src/skill.rs` that keep the stricter staging contract present in bundled instructions.

## Files To Inspect

- `src/agent-doc/src/skill.rs`
- `src/agent-doc/SKILL.md`
- `src/agent-doc/runbooks/commit.md`
- `src/agent-doc/runbooks/harness-invocation.md`
- `src/agent-doc/runbooks/compound-task-steering.md`
- `src/agent-doc/src/git.rs`
- any current repo-work helper surfaces that stage/commit narrowed path sets for `commit + push`

## Acceptance Criteria

1. A narrowed manual repo commit in an `agent-doc` turn cannot continue after a failed stage step.
2. The workflow must prove that the staged diff matches the intended path set before commit.
3. Unrelated pre-existing staged entries cannot be silently consumed by a later narrowed commit.
4. Submodule-pointer updates remain supported as a first-class case.
5. Regression coverage exists for the failed-stage + dirty-index repro.
