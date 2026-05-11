# SessionShare Root Response Uncommitted Plan

## Problem

The latest response for `src/session-share/tasks/root.md` was written into the
session document but the document remained dirty afterward. The visible
patchback includes the BuildParty pause-regression closeout:

- BuildParty commit: `2336083`
- Session document: `src/session-share/tasks/root.md`
- Symptom: `git -C src/session-share status --short` reports
  `M tasks/root.md` after the response claims the work was done, verified,
  committed, and pushed.

This is the patched-but-uncommitted variant of the response closeout bug: the
assistant response is present in the document, but the commit boundary did not
complete.

## Desired Behavior

After a successful response patchback, agent-doc must either:

1. commit the session document in its owning repository and commit any required
   parent submodule pointer, or
2. fail closed with an explicit recovery command before reporting success.

`session-check` should classify this as an incomplete closeout, not as a normal
dirty working tree.

## Fix Plan

1. Add this SessionShare `tasks/root.md` case as a regression fixture for the
   response-patchback commit boundary.
2. Extend closeout/session-check diagnostics to distinguish:
   - response patchback missing,
   - response patchback visible but target document uncommitted,
   - target document committed but parent submodule pointer uncommitted.
3. Ensure direct Codex chat/preset flows that write a session response run the
   same final commit/session-check boundary as `agent-doc finalize`.
4. Make recovery idempotent: rerunning `agent-doc commit <file>` should commit
   only the missing layer and should not duplicate the response body.

## Regression Coverage

- Simulate a response patchback applied to a submodule-owned session document
  where the target repo remains dirty; assert closeout fails.
- Simulate a successful target-document commit with a stale parent submodule
  pointer; assert closeout names the parent pointer as the missing layer.
- Simulate recovery after the response body is already visible; assert no
  duplicate exchange block or response heading is produced.

## Non-goals

- Do not change response text formatting.
- Do not broaden automatic cleanup of unrelated dirty files.
- Do not treat unrelated user edits as safe to commit.
