# Plan: fail closed when response patchback remains uncommitted

## Problem

A session-document response patchback can be visible in the working tree while
the closeout path has not committed the owning repository state. The user then
has to notice the dirty patchback and ask for a manual commit.

Current evidence:

- SessionShare root turn: `src/session-share/tasks/root.md`
- User report: "The response patchback is uncommitted."
- Manual recovery command:
  `agent-doc commit /home/brian/work/btakita/agent-loop/src/session-share/tasks/root.md`
- Recovery output said the target document's staged snapshot already matched
  `HEAD`, but the parent submodule pointer was stale and had to be committed
  afterward.

This overlaps with older patchback/closeout drift plans, but the invariant here
is simpler: after a response patchback is reported complete, the owning repo
and any required parent submodule pointer must either be clean and committed or
the command must fail closed with an explicit recovery command.

## Desired Behavior

1. A successful agent-doc turn that writes a response patchback must also finish
   the commit boundary for:
   - the document's owning repository
   - parent repositories whose submodule pointers changed
2. If the closeout cannot commit any required layer, it must fail closed before
   reporting success.
3. `agent-doc session-check` must detect visible response patchbacks that lack a
   matching committed closeout and name the exact missing layer.
4. Running `agent-doc commit <file>` as recovery should be idempotent and should
   explain whether it committed:
   - target document content
   - snapshot repair only
   - parent submodule pointer only

## Investigation Steps

- [ ] Reconstruct the SessionShare root closeout chain that produced a visible
      response patchback but still needed a later manual commit.
- [ ] Identify whether the missing boundary was the target document commit,
      snapshot repair, parent submodule pointer commit, or editor-side working
      tree drift.
- [ ] Compare this shape with:
      `plan-preflight-started-patchback-closeout.md`,
      `plan-buildparty-write-stream-closeout-gap.md`, and
      `plan-claudescore-3-turn-complete-drift.md`.
- [ ] Decide whether this should become a new invariant in `session-check` or
      an extension of an existing closeout invariant.

## Implementation Steps

- [ ] Add a closeout finalizer check that verifies all required owning repos are
      clean or have been committed before success is returned.
- [ ] Teach submodule-owned document closeout to report parent pointer commits as
      first-class closeout work, not incidental post-processing.
- [ ] Add diagnostics that distinguish "target doc already committed" from
      "parent pointer still dirty."
- [ ] Add a recovery path that can safely finish only the missing layer without
      replaying or duplicating the assistant response.

## Regression Coverage

- [ ] Simulate a submodule-owned document response where the target repo commit
      succeeds but the parent pointer remains dirty; closeout must not report
      full success.
- [ ] Simulate a visible response patchback with no target repo commit; next
      `session-check` must fail closed and prescribe `agent-doc commit <file>`.
- [ ] Simulate idempotent recovery after the target snapshot already matches
      `HEAD`; recovery commits only the stale parent pointer and reports that
      precisely.

## Exit Criteria

- No agent-doc turn can leave a visible response patchback uncommitted while
  reporting success.
- Manual `agent-doc commit <file>` recovery remains safe, but should not be
  required after a normal successful response.
