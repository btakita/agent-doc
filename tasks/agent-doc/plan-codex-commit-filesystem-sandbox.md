# Plan - fix Codex commit filesystem sandbox denial

**Pending:** `#commitfs`
**Status:** Draft

## Problem

An agent-doc-managed Codex session can edit files in a nested submodule working
tree, then fail when it tries to commit because the real git metadata lives
under the parent repository `.git/modules/...` directory outside the effective
filesystem sandbox.

Current evidence from `src/session-share/tasks/root.md`:

```text
fatal: Unable to create '/home/brian/work/btakita/agent-loop/.git/modules/src/session-share/modules/build-party/index.lock': Read-only file system
```

The user-facing symptom was: "Commit is blocked by the filesystem sandbox."
That is materially different from an application test failure or an ordinary
stale `index.lock`: the working tree was writable, but the commit boundary was
not.

Related work:

- `tasks/agent-doc/plan-submodule-gitdir-permission-issue.md` records an older
  nested-submodule gitdir failure that appeared fixed for fresh sessions.
- `tasks/agent-doc/plan-codex-managed-session-capability-proof.md` covers the
  broader managed-session capability proof invariant.
- `tasks/agent-doc/plan-false-writable-root-refusal.md` covers diagnostic
  wording when writeability, gitdir access, and network/push capability are
  conflated.

This plan tracks the fresh commit-specific regression and should either produce
a code fix or merge into the broader capability-proof work with this repro as a
required regression.

## Desired Behavior

1. Before dispatching work that can end in `commit + push`, agent-doc proves the
   live Codex child can write the git metadata required for each affected repo
   and nested submodule.
2. Writable working-tree access is not treated as proof of commit capability.
3. Gitdir access covers:
   - the repo `.git` path or gitfile target
   - parent `.git/modules/...` entries for submodules
   - nested `.git/modules/...` entries for submodules inside submodules
   - the superproject index when a submodule pointer update is required
4. If the managed pane lacks gitdir write access, agent-doc restarts/rebinds the
   pane once with the complete writable-root contract and re-probes before
   dispatch.
5. If the capability still fails, agent-doc fails closed before doing more work
   and reports the exact blocked git metadata path.

## Investigation Steps

- [ ] Reproduce the current `build-party` failure from a fresh agent-doc-managed
      Codex session, not only from a reused pane.
- [ ] Capture the effective writable roots, `--add-dir` list, gitfile targets,
      and resolved `commondir`/`gitdir` paths for:
  - `src/session-share`
  - `src/session-share/build-party`
  - the parent `agent-loop` superproject
- [ ] Compare the outer Codex/session shell capability with the managed Codex
      child capability for the exact blocked `index.lock` path.
- [ ] Audit the current workspace access calculation and confirm whether the
      `build-party` gitdir is missing, inherited but not effective, or lost
      during route/reuse/resume.
- [ ] Check whether direct manual commit flows and binary-owned
      `finalize`/`write --commit` flows use the same gitdir capability proof.

## Implementation Steps

- [ ] Add a git metadata capability probe for Codex-managed sessions that tests
      write access to the resolved index directory without leaving lock files
      behind.
- [ ] Include resolved submodule gitdirs and required superproject gitdirs in
      the launch/reuse writable-root contract.
- [ ] Invalidate stale proofs after `/clear`, resume, controller handoff, actor
      generation change, and any read-only filesystem/index-lock diagnostic.
- [ ] Restart once on failed gitdir proof when the launch contract should grant
      access, then re-probe before dispatch.
- [ ] Improve diagnostics so read-only git metadata is reported as commit
      capability denial, not as stale lock contention or generic sandbox
      failure.
- [ ] Cross-link or merge this item into the broader capability-proof plan once
      the build-party repro is covered by tests.

## Regression Coverage

- [ ] A nested submodule whose working tree is writable but parent
      `.git/modules/...` gitdir is read-only fails before commit work with a
      precise gitdir capability diagnostic.
- [ ] A managed Codex pane with correct visible launch args but missing gitdir
      write access is not reused blindly.
- [ ] A fresh restart that gains gitdir access clears the failure marker and
      allows the commit path to proceed.
- [ ] A fresh restart that still lacks gitdir access fails closed with the
      blocked path.
- [ ] Submodule pointer updates prove both the submodule gitdir and the
      superproject index path.

## Exit Criteria

- `agent-doc session status` can report whether the active Codex actor has
  proven git metadata write capability for the repos touched by the document.
- Commit-oriented workflows no longer reach the end of implementation before
  discovering that `.git/modules/.../index.lock` is read-only.
- The root/build-party repro is covered by a regression test or documented as a
  required capability-proof fixture.
