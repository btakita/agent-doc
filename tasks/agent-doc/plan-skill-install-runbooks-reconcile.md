# plan: agent-doc skill install reconciles target runbooks directory

## Problem

`agent-doc skill install --harness claude` writes new files but does not delete stale ones. Over time, the target `.claude/skills/agent-doc/runbooks/` directory accumulates files that no longer exist in the binary's embedded set, and (separately) misses new files added to the source after the last install completed.

Concrete current-state evidence (`agent-loop` working tree, 2026-05-25):

- `agent-loop/.claude/skills/agent-doc/runbooks/`: **21 files**
- `src/agent-doc/runbooks/` (source of truth): **14 files**
- Drift:
  - **8 stale extras** in the installed copy: `cleanup.md`, `diff-type-routing.md`, `investigate-behavior.md`, `large-content.md`, `multi-topic-response.md`, `plugin-install.md`, `session-share-commit.md`, `skill-submit-cycle.md`. None of these still exist in the source.
  - **1 new source file never installed**: `manual-job-packets.md`. The install short-circuited on "SKILL.md already up to date" and did not run the runbooks pass at all.

## Impact

- Stale runbooks are loaded into harness instructions and continue to bias agent behavior toward removed/deprecated workflows.
- New runbooks committed to source never reach harnesses that already have an up-to-date SKILL.md.
- The fact that the agent-loop installed copy has the *wrong number* of runbooks is invisible to anyone who only checks SKILL.md version.

## Root cause

`src/skill.rs` install logic:

1. Compares the installed `SKILL.md` to the binary's embedded `SKILL.md`. If they match, short-circuits as "already up to date" — including the runbooks pass.
2. When it does run the runbooks pass, it writes the embedded runbooks to disk one by one but never enumerates the existing directory and reaps files that are no longer in the embedded set.

## Fix

Two coordinated changes:

1. **Always run the runbooks pass.** SKILL.md being up to date is not evidence that runbooks are. Separate the SKILL.md check from the runbooks check; only short-circuit when both the SKILL.md hash and the runbook set match.
2. **Reconcile the runbooks directory.** Compute the canonical filename set from the binary's embedded content, enumerate the target directory, and either:
   - **Delete-then-write** (simplest): clear `runbooks/` of any `.md` files not in the canonical set before writing, OR
   - **Reap-on-mismatch** (gentler): compute the diff between canonical and on-disk, write missing files, delete stale ones, leave matching files alone. Prefer this — avoids touching file mtime / VCS noise for unchanged content.

## Scope

- File: `src/skill.rs` (install path)
- File: `src/skill_content.rs` or wherever runbook embeds live (whichever owns `include_str!` / `include_bytes!` for runbooks)
- Tests: a new test that asserts post-install on-disk runbook set == canonical set, including a setup case that pre-populates `runbooks/foo-stale.md` and asserts it gets reaped.
- Harness coverage: the same fix logic applies for `--harness codex` (Codex AGENTS.md + runbooks) and `--harness opencode` if those harnesses install runbooks too. Verify which other harness paths need the same change.

## Out of scope

- Touching the runbook *content*. This plan is about the install workflow, not the runbook authoring layer.
- Changing how runbooks are embedded in the binary. The current `include_str!` (or similar) pipeline stays.

## Non-goals

- Backward compatibility for users who hand-edited their installed runbooks. This is install-managed content; hand edits get overwritten today and will continue to.

## Verification

After the fix:

1. `agent-doc skill install --harness claude` on a project with 21 stale + 1 missing runbook should converge `runbooks/` to exactly the 14 source files.
2. Re-running install on a clean tree should be a no-op (no file touched, no mtime change).
3. A new test that pre-populates `runbooks/sentinel-stale.md` then runs install and asserts the sentinel is gone.

## Tracking

- Surfaced from: `tasks/software/tagpath.md` cycle `#p6sklsymadocrunbooks` (decision to leave the runbooks subtree install-owned rather than symlinked).
- Backlog home: `tasks/agent-doc/agent-doc-bugs2.md` — id `#p6skillinstallreconcile`.
