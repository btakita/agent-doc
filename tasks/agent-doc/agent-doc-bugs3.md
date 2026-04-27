# agent-doc bugs 3

## Status

Tracking the `claudescore-3.md` post-turn closeout-bypass bug separately from the older missed-trigger and interrupted-commit families. In this case, the last binary-owned `agent-doc` patchback committed successfully, but later edits to the active session document were recorded as `out_of_band_write` / `post_commit_local_drift` and left dirty without a new `write --commit` / `finalize` cycle.

## Backlog

- [ ] [#closeout-bypass] Fix active-session post-turn closeout bypass for `src/session-share/tasks/claudescore-3.md`: if the target session document changes after the last committed patchback during the same assistant turn, `agent-doc` must reopen closeout or fail closed instead of classifying the change as harmless local drift. Plan: `tasks/agent-doc/plan-claudescore-3-post-turn-closeout-bypass.md`
