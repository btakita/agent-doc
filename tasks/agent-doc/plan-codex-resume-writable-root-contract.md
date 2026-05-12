# Plan - enforce Codex writable-root proof on resume and reuse

**Pending:** `#codexresumegitdir`
**Status:** Implemented 2026-05-12

## Problem

The May 12 Monster Rod Holders shipping-rule turn showed a remaining sandbox
gap after the earlier nested-submodule gitdir fix. The agent could edit
`src/boost-client/src/monsterrodholders-dev`, but reported that the local
submodule checkout could not update backing git metadata under
`.git/modules/...`, so it committed from a temporary clone and left the local
submodule worktree dirty.

The existing fresh-launch work adds nested `.git/modules/...` paths to Codex
`--add-dir` roots, but that is not sufficient for reused or resumed Codex
actors. Current code intentionally strips `--add-dir` from `codex exec resume`
because resume inherits roots from the original session. If the original actor
was started before the complete root set was known, or if capability proof only
ran against a separate probe child, `agent-doc` can still dispatch real work to
a pane that cannot commit locally.

## Desired Behavior

1. A Codex actor is reusable only if the actor generation has proven the exact
   writable-root contract required by the current document and likely touched
   repos.
2. Required writable roots include:
   - the document repo or submodule working tree
   - resolved `.git` or gitfile target
   - nested `.git/modules/...` gitdirs
   - the parent superproject gitdir/index when a submodule pointer update is
     required
3. `codex exec resume` must not be used as a way to add missing roots. If roots
   are missing or unproven, `agent-doc` must fresh-start/rebind the actor with
   the full `--add-dir` set, then prove it.
4. Capability proof must run against the actor that will receive the task, not
   only against a separate short-lived Codex probe process.
5. If proof still fails, `agent-doc` fails closed before implementation work and
   reports the blocked git metadata path plus the required restart/rebind action.

## Implementation Steps

- [x] Extend managed Codex proof events with a `writable_root_contract`
      fingerprint keyed to the normalized required root set.
- [x] Compute the required root set from the effective Codex args plus
      `git::workspace_access_dirs_for_doc()`.
- [x] On route/start/reuse, compare required roots with the current actor proof.
      If roots are missing or proof belongs to an older generation/root set, do
      not dispatch.
- [x] For direct Codex backend resume, keep stripping `--add-dir`, but fresh-start
      with complete `--add-dir` args whenever the current launch contract
      requires writable roots.
- [x] Keep the existing bounded writable-root probe for launcher and Codex child
      create/remove checks, including representative `index.lock` checks in git
      metadata dirs.
- [x] Invalidate writable-root proof through the existing latest-`session_start`
      boundary and root-contract comparison.
- [x] Keep git metadata denial diagnostics explicit through the writable-root
      child probe classification.

## Regression Coverage

- [x] Reused Codex actor whose visible args imply `--add-dir` but whose current
      generation lacks gitdir proof is not dispatched; route fresh-starts or
      fails closed.
- [x] `codex exec resume` with newly required `.git/modules/...` roots does not
      silently proceed after stripping `--add-dir`.
- [x] Fresh-start with complete roots proves the same launch contract can create/delete a
      git metadata lock probe and then permits commit-oriented work.
- [x] Nested submodule plus parent-pointer update requires both child gitdir and
      superproject gitdir/index proof.
- [x] Monster Rod Holders shape: document in `src/boost-client/tasks`, work in
      nested `src/boost-client/src/monsterrodholders-dev`, parent pointer in
      `boost-client`, and outer pointer in `agent-loop`.

## Exit Criteria

- The Monster Rod Holders shipping-rule workflow would either run in a Codex
  actor that can update the local submodule gitdir and parent pointer, or stop
  before edits with a precise capability error.
- Temporary-clone commits are no longer needed to work around missing local
  `.git/modules` access in managed Codex sessions.
- `agent-doc session status <FILE>` can show the current actor's proven writable
  roots and whether they satisfy the active document.
