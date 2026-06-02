# Plan: JB Compact Exchange repair can leave correction uncommitted

**Bug ID:** `#jb-compact-repair-left-uncommitted`
**Status:** planned
**Observed:** 2026-05-30 in `src/boost-client/tasks/monsterrodholders.md`

## Problem

After the malformed `Compact Exchange` result, the document had an uncommitted
correction. A repair that normalizes the compacted exchange but does not cross a
binary-owned commit boundary leaves the session in a dangerous intermediate
state:

- the visible editor contains the corrected exchange shape
- `HEAD` still contains the malformed compact result
- later JetBrains actions see mixed visible/HEAD/snapshot state and can route or
  queue from the wrong surface

The user expectation is that a repair invoked from JetBrains either commits the
correction through the same validated agent-doc path or fails closed with a
single explicit command to run. It should not silently leave a repaired document
dirty.

## Desired Behavior

For compact repair and compact-output normalization:

- successful repair must update the snapshot and commit the corrected document
- if there are unrelated user edits, the repair must stop before mutation or
  preserve them with an explicit mixed-edit diagnostic
- JetBrains should present "repair applied and committed" or "repair blocked"
  as distinct outcomes
- route/preflight should not treat an uncommitted compact repair as an active
  prompt or queue source

## Plan

1. Reconstruct the repair path that produced the current dirty correction in
   `src/boost-client/tasks/monsterrodholders.md`.
2. Identify whether the repair came from `agent-doc repair`, `write --commit`,
   `compact exchange`, or a JetBrains-side editor normalization.
3. Add a repair-closeout invariant:
   - if repair changes only agent-doc-owned compact/exchange structure, commit
     it atomically
   - if repair sees user-authored dirty edits, fail closed with exact paths and
     recovery command
4. Teach route/preflight to classify uncommitted compact repair separately from
   new prompt input.
5. Add a JetBrains notification test so the IDE cannot report a repair as done
   while the session doc remains dirty.

## Likely Files

- `src/agent-doc/src/repair.rs`
- `src/agent-doc/src/git.rs`
- `src/agent-doc/src/preflight.rs`
- `src/agent-doc/src/route.rs`
- JetBrains action code for Compact Exchange / repair notifications

## Verification

- Regression: malformed compact output repaired by the binary produces a commit
  and clean worktree.
- Mixed-edit regression: repair plus unrelated user edit fails closed without
  losing the user edit.
- Live JetBrains replay: Compact Exchange repair does not leave
  `tasks/monsterrodholders.md` dirty after success.
