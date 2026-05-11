# Plan: Valid OOB Tail Deletion Is Misclassified As Local Drift

**Pending:** `#oobtaildel` in `tasks/agent-doc/agent-doc-bugs2.md`
**Status:** Planned

## Problem

When a user deletes malformed prompt content that lives outside `<!-- agent:exchange -->`, `agent-doc commit` can classify the edit as generic post-commit local drift and close the active cycle as already committed.

That is wrong for this shape: the deleted content is not a new user follow-up that should remain dirty; it is valid cleanup of conversation text that previously escaped the exchange component.

## Current Evidence

- `src/session-share/tasks/root.md` had prompt content after `<!-- /agent:exchange -->`.
- The user deleted that escaped tail and confirmed the deletion was valid.
- `agent-doc .../tasks/root.md` reported:
  - `detected post-commit local drift`
  - `staged snapshot already matches HEAD`
  - left the target document dirty
- A naive fix that only calls `strip_conversation_tail_outside_exchange(HEAD)` is insufficient because the real document can have mixed edits: escaped tail removal plus a fresh prompt inside `agent:exchange`.

## Desired Behavior

- Valid deletion of escaped conversation content outside `agent:exchange` is recognized as cleanup, not generic local drift.
- If the same live diff also contains a fresh unresolved prompt inside `agent:exchange`, the prompt remains the next executable user input and must not be swallowed by cleanup adoption.
- The commit/closeout path must either:
  - commit only the safe cleanup when no new prompt is present, or
  - fail closed / preserve the prompt for the next cycle with a precise diagnostic.
- Non-conversation notes outside components remain protected user edits and must not be auto-deleted or auto-committed.

## Implementation Plan

- Add a regression using the `tasks/root.md` shape:
  - HEAD has escaped prompt/image text after `<!-- /agent:exchange -->`.
  - working tree deletes that escaped tail.
  - variant includes a new prompt inside `agent:exchange`.
- Extend post-commit drift classification to distinguish:
  - transient marker drift
  - valid escaped-tail cleanup
  - real later user follow-up
  - unrelated working-tree edits
- Reuse `template::strip_conversation_tail_outside_exchange` where it matches, but add a diff-aware fallback for prompt-prelude tails that start with descriptive prose before the directive.
- Keep cleanup adoption narrow:
  - only after the exchange close marker
  - never overlapping any `agent:*` component
  - only component-free prose/media/preset text that looks like escaped exchange content
- Add session-check coverage so the direct Codex path cannot report success while leaving this shape as unexplained dirty drift.

## Files Likely To Change

- `src/agent-doc/src/git.rs`
- `src/agent-doc/src/template.rs`
- `src/agent-doc/src/session_check.rs`
- `src/agent-doc/tests/finalize_integration.rs`
- `src/agent-doc/specs/07-closeout-commands.md`

## Exit Criteria

- The root-doc reproduction no longer logs generic `post_commit_local_drift` for a valid escaped-tail deletion.
- Mixed cleanup plus new prompt does not lose the prompt.
- Existing post-commit local drift tests still prove unrelated edits remain uncommitted.
