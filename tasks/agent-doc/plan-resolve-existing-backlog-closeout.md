# Plan: Resolve Existing Backlog Items on Closeout

**Status:** Proposed  
**Date:** 2026-04-27  
**Relates to:** `session-check`, `finalize`, pending/backlog mutations, `tasks/software/tsift.md`

---

## Problem

A response can clearly complete an existing backlog item, yet the item remains open if the cycle does not record the matching `--pending-done <id>` mutation.

That happened in `tasks/software/tsift.md` for `#vxtz`:

- the response said the work was implemented, verified, committed, and pushed
- `session-check` recognized that shape and warned that `#vxtz` looked complete
- the cycle still committed successfully, so the backlog item stayed open until a later manual repair turn

This is a bad state for `agent-doc`: the binary already knows enough to identify the unresolved backlog mutation, but the closeout path only warns instead of deterministically closing or failing the turn.

---

## Goal

For existing backlog items that the current response clearly resolves, `agent-doc` should do one of two things before reporting success:

1. apply the matching `pending-done` mutation in the same closeout path, or
2. fail closed and require the caller to retry with the explicit mutation

It should not commit a response that semantically closes the item while leaving the backlog stale by default.

---

## Design

### Phase 1: tighten the closeout guard

When `session-check` or pre-closeout analysis detects that a response resolves an existing backlog item, upgrade the current warn-only path to a strict closeout failure for active session docs.

Expected behavior:

- if the cycle already recorded `--pending-done <id>`, proceed normally
- if not, fail the closeout with a phase-labelled error that names the missing item id
- keep the cycle resumable so a repair command can replay the same response with the missing pending mutation

Files to inspect:

- `src/agent-doc/src/plan.rs`
- `src/agent-doc/src/finalize.rs`
- `src/agent-doc/src/session_check.rs`
- `src/agent-doc/tests/finalize_integration.rs`

### Phase 2: optional binary-owned auto-resolution

If the response/item match is unambiguous, allow the binary to convert that signal into the equivalent `pending-done` mutation automatically before commit.

Scope constraints:

- only for existing stable backlog ids already present in the target document
- only when the response clearly claims completion of that exact id
- leave ambiguous recommendation/proposal text on the existing explicit-mutation path

---

## Tests

- response completes `#id` and includes `--pending-done id` -> closeout succeeds and backlog item is marked done
- response completes `#id` without `--pending-done id` -> closeout fails closed instead of warn-and-commit
- repair path can resume the captured response with `--pending-done id`
- non-completion references to a backlog id do not trigger the guard

---

## Result

This closes the specific stale-backlog class behind `#vxtz`: "response committed successfully, backlog item still open, `session-check` only warned".
