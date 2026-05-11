# Late Fallback Patch Re-dirties Committed Doc Plan

## Problem

After a successful agent-doc closeout for `src/session-share/tasks/root.md`, the
session document became dirty again. The late working-tree diff contained:

- a corrupted already-committed heading:
  `#vt-semantic-hero-scree nshot`
- a duplicate stale response block:
  `### Re: #vt-semantic-hero-screenshot sheet import - codex (HEAD)`
- `agent-doc session-check tasks/root.md` then failed with a direct-response
  patchback warning even though the valid repo-push closeout had already been
  committed.

This looks like delayed IPC/file fallback patch delivery after the successful
finalize/commit path. The late patch reintroduced older unmatched content into
the exchange body and made the document appear uncommitted.

## Desired Behavior

Once a closeout cycle reaches committed success, any delayed IPC/file fallback
patch for the same cycle must be rejected, ignored, or idempotently recognized as
already applied. It must not mutate the working tree after the commit boundary.

## Fix Plan

1. Reproduce with a fixture based on the `root.md` cycle that produced:
   - a committed repo-push response,
   - a delayed stale fallback patch,
   - a post-commit dirty exchange diff.
2. Add a cycle/patch identity check before applying fallback patches:
   - reject patches older than the committed cycle state,
   - reject patches whose boundary target is no longer current,
   - reject duplicate response bodies already present in the committed exchange.
3. Make late fallback rejection explicit in logs with patch id, cycle id, and
   document path.
4. Add a regression that runs `session-check` after the simulated late patch and
   verifies the document remains clean and committed.
5. Audit existing sidecar/fallback retry paths for any code that can apply a
   patch after commit success without re-reading current HEAD/worktree state.

## Non-goals

- Do not change the response content synthesis format.
- Do not broaden duplicate-response cleanup as a substitute for preventing the
  late write.
