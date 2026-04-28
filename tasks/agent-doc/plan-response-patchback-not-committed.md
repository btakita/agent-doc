# Plan — fix response patchback left uncommitted

**Pending:** `#patchcommit`
**Status:** Implemented

## Problem

A response patchback can visibly land in the target document while the owning repo or submodule remains dirty because no binary-owned closeout commit follows. The user sees the correct content in the file, but the patchback was not actually committed as part of the response cycle.

This is narrower than the older missed-trigger family:

- the response patchback is already present in the working file
- the failure is that closeout ends without committing the owning repo/submodule
- rerunning later should not depend on a human noticing and committing the patch manually

The `src/session-share/tasks/claudescore-3.md` repro is the reference case for this plan.

## Goal

Ensure that once a response patchback is visible in the target document, `agent-doc` either:

- completes the owning repo/submodule commit in the same closeout path, or
- fails closed with resumable state and a precise diagnostic instead of reporting success while the target doc stays dirty.

## Desired Behavior

- visible response patchback + dirty owning repo/submodule is never treated as a clean successful closeout
- active session docs in submodules use the correct owning git root for the commit requirement
- post-write / post-response drift on the target doc is classified precisely:
  - still part of the current assistant turn and must reopen closeout, or
  - true later human/local drift and must remain dirty with a clear reason
- `session-check`, `preflight`, and repair surfaces can explain whether the patchback:
  - never reached commit
  - reached commit in the wrong repo/root
  - was followed by binary-owned post-commit mutation that reopened the target doc

## Implementation Steps

- [ ] Reconstruct the exact `claudescore-3.md` failure chain from local `agent-doc` evidence:
  - visible response patchback exists in the file
  - session-share submodule remains dirty
  - the last binary-owned closeout marker does not prove the target doc commit happened
- [ ] Trace the closeout path in `src/agent-doc` for active session documents:
  - write / patchback application
  - owner-repo detection for nested repos and submodules
  - finalize / commit dispatch
  - post-commit success classification
- [ ] Define the invariant:
  - if the target doc changed during the assistant turn, success requires either a matching owning-root commit or an explicit fail-closed state
- [ ] Distinguish three states explicitly in code and diagnostics:
  - patchback present, commit missing
  - patchback committed, later binary-owned mutation reopened the file
  - patchback committed, later human edit created legitimate next-turn drift
- [ ] Tighten success/clean checks so a response cycle cannot exit green while the target doc is still dirty in its owning repo/submodule
- [ ] Add deterministic recovery behavior:
  - reopen closeout / resume commit when the dirty state still belongs to the just-finished assistant turn
  - fail closed when ownership or drift provenance is ambiguous
- [ ] Verify nested-repo behavior explicitly:
  - target doc inside a submodule must require cleanliness/commit proof for the submodule
  - unrelated dirt in the parent repo must not block a successful submodule-owned closeout
- [ ] Add regression coverage for:
  - normal response patchback commits the owning submodule and exits clean
  - visible patchback with missing commit fails closed
  - post-commit binary-owned rewrite on target doc reopens closeout instead of reporting success
  - later human edit after successful closeout does not get auto-committed retroactively

## Files Likely To Change

- `src/agent-doc/src/finalize.rs`
- `src/agent-doc/src/preflight.rs`
- `src/agent-doc/src/session_check.rs`
- `src/agent-doc/src/write.rs`
- `src/agent-doc/src/repair.rs`
- `src/agent-doc/src/git.rs`
- `src/agent-doc/specs/07-commands.md`

## Exit Criteria

- a visible response patchback cannot be left uncommitted while `agent-doc` reports successful closeout
- the owning repo/submodule for the target document is checked correctly
- rerunning after this failure either resumes the closeout deterministically or fails closed with a concrete reason
