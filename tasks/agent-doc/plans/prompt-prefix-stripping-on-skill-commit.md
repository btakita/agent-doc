# Plan: Prompt prefix stripping on skill-level commit

## Bug

When a skill runs a compound task like `spec-test-news-commit-push`, the skill commits repo work (spec updates, tests, news) via `git add` + `git commit` before `finalize` runs. If the session document is staged in that commit, the user's raw prompts are committed **without** the `❯ ` prefix — those prefixes are only added by `finalize`. The intermediate commit in git history has bare prompt lines.

`finalize` subsequently fixes the prefixes in a separate commit, so the final HEAD state is correct. But the git history contains a commit where prompt integrity is violated.

## Reproduction

1. User edits a session document, adding a prompt like `spec-test-news-commit-push`
2. Skill performs repo work (code changes, spec updates, news entries)
3. Skill runs `git add <session-doc> <other-files> && git commit` to capture all work
4. User's raw prompts are committed without `❯ ` prefix
5. Skill runs `agent-doc finalize` which writes the response and fixes prefixes
6. Git history has two commits: one with bare prompts, one with repaired prefixes

Evidence: commit `d4884e09` in `ClaudeScore/SessionShare` — line 135 has bare `spec-test-news-commit-push` without `❯ ` prefix. Fixed in the subsequent `41dbc431` finalize commit.

Prior instance: same class of bug reported in `agent-doc-bugs2.md` line 234 ("the following had an uncommitted change that stripped out the `❯ ` prefix"), previously traced to JetBrains/VS Code normalize order (fixed in `362bcd1`). This instance is different — it's the skill-level commit path, not an editor plugin.

## Root Cause

Two gaps:

1. **Skill instructions do not exclude the session document from non-finalize commits.** The compound task `spec-test-news-commit-push` triggers code changes + commit + push as a single flow. When the session document is in the same repo as the code being committed, `git add` captures it with raw user edits.

2. **`agent-doc commit` does not validate prompt prefix integrity before committing.** There is no pre-commit guard that fails closed when a session document has bare prompt-bearing lines without the `❯ ` prefix.

## Proposed Fix

### Layer 1: Skill-level guard (instructions)

Update the agent-doc skill instructions (CLAUDE.md / SKILL.md) to explicitly state:

> **Do not stage or commit the session document in any commit other than the `finalize` / `write --commit` code path.** When running compound tasks that include `commit + push`, exclude the session document file from staging. The session document is only committed through `agent-doc finalize`.

This prevents the symptom but relies on the skill following instructions.

### Layer 2: Pipeline-level guard (agent-doc binary)

Add a pre-commit validation in `agent-doc commit` (or the `git::commit()` pathway):

1. When staging a session document for commit, scan prompt-bearing lines for missing `❯ ` prefix
2. If bare prompt lines are found and this is NOT a `finalize` / `write --commit` code path, fail closed with an error message like: `session document has bare prompt lines without ❯ prefix — use agent-doc finalize instead of direct git commit`
3. The `finalize` path already applies prefix normalization before commit, so it would pass this check

Alternatively, add a `session-check`-style guard to the `agent-doc commit` entry point that validates prompt prefix integrity as part of the commit precondition.

### Layer 3: Compound task ordering (optional)

Consider whether compound tasks like `spec-test-news-commit-push` should defer ALL commits to `finalize`. Instead of: code changes → commit → push → finalize, the flow would be: code changes → finalize (which commits the response + all staged code changes) → push. This would eliminate the intermediate commit entirely.

## Files to Investigate

- `src/agent-doc/src/git.rs` — `commit()` function, staging logic
- `src/agent-doc/src/preflight.rs` — `repair` step that fixes prompt prefixes
- `src/agent-doc/src/template.rs` — prompt prefix normalization
- `src/agent-doc/src/session_check.rs` — existing prompt-prefix closeout guard
- Skill instructions: `CLAUDE.md`, `~/.claude/skills/agent-doc/SKILL.md`

## Acceptance Criteria

1. A session document with bare prompt-bearing lines (missing `❯ ` prefix) cannot be committed through a non-finalize code path
2. The guard is fail-closed: it blocks the commit with an actionable error, not a silent repair
3. `finalize` and `write --commit` continue to work normally (they normalize prefixes before commit)
4. Regression test covers the scenario: bare-prompt session doc staged alongside code changes in a direct `git commit`
