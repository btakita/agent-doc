# Plan: Preserve Comment Tail Edits After Exchange

**Pending:** `#commenttail` in `tasks/agent-doc/agent-doc-bugs2.md`
**Status:** Implemented

## Problem

A user edit inside the plain HTML comment immediately below `<!-- /agent:exchange -->` was lost at the end of an agent-doc closeout cycle.

That comment is outside every `agent:*` component, but it is still live user-authored document content. Response closeout, boundary reposition, snapshot staging, and editor IPC refresh must not delete or overwrite it.

## Current Evidence

- The affected shape in `tasks/agent-doc/agent-doc-bugs2.md` is:
  - `<!-- /agent:exchange -->`
  - a plain `###` separator
  - a plain HTML comment containing parked prompts/notes
  - `agent:backlog`
- A quick disk-path probe did **not** reproduce deletion: a concurrent edit to a comment between `agent:exchange` and `agent:pending` survived `finalize --stream`.
- That same probe showed the late non-component edit can be absorbed into the closeout commit, so the ownership boundary for out-of-component edits is also suspect.
- The more likely deletion path is editor/IPC closeout:
  - `try_ipc_reposition_boundary` sends a post-commit reposition event.
  - the JetBrains plugin may choose `diskContent` over `document.text` for boundary cleanup when it believes the editor differs only by transient markers.
  - any stale disk snapshot or reload at that point can clobber comment-only edits outside components.

## Goal

Make all closeout cleanup paths preserve user edits outside `agent:*` components, especially HTML comments directly below `agent:exchange`.

## Desired Behavior

- Late edits in non-component comment tails remain in the editor buffer and working tree.
- Those edits are not silently committed as part of the assistant response unless the user explicitly requested committing that document edit.
- Boundary reposition and `(HEAD)` cleanup may touch the `agent:exchange` component but must preserve bytes outside the component unchanged.
- If the binary/plugin cannot prove the editor buffer is only transiently different from disk, it must use the editor buffer as the source of truth.

## Implementation Steps

- [x] Add Rust integration coverage for `finalize --stream` with a comment tail directly below `agent:exchange`, including the `###` separator shape from `agent-doc-bugs2.md`.
- [x] Decide and encode whether late non-component edits should remain uncommitted after closeout; current disk behavior appears to absorb them into the response commit.
- [x] Add JetBrains unit coverage for `repositionBoundaryViaDocument` decision helpers:
  - comment-tail edit present only in editor content
  - disk content has the committed response and fresh boundary
  - editor content has stale/transient boundary markers plus the comment-tail edit
  - `shouldPreferCommittedDiskContentForRepositionUtil(...)` must return false
- [x] Audit `NativePatching.repositionBoundaryToEnd*` and Kotlin fallback `repositionBoundaryToEndUtil(...)` to prove they preserve all bytes before the exchange opener and after the exchange closer.
- [x] Tighten the disk-vs-editor preference rule so comment-tail, markdown-tail, and other non-component edits always block disk preference.
- [x] Add simulated reposition coverage for a post-commit boundary cleanup shape with a comment-tail edit.
- [x] Update closeout docs/specs to state that non-component user edits are preserved and are outside assistant-response ownership.

## Files Likely To Change

- `src/agent-doc/src/write.rs`
- `src/agent-doc/src/git.rs`
- `src/agent-doc/tests/finalize_integration.rs`
- `src/agent-doc/editors/jetbrains/src/main/kotlin/com/github/btakita/agentdoc/PatchWatcher.kt`
- `src/agent-doc/editors/jetbrains/src/test/kotlin/com/github/btakita/agentdoc/RepositionBoundaryTest.kt`
- `src/agent-doc/specs/07-closeout-commands.md`

## Exit Criteria

- Reproductions that edit the comment below `agent:exchange` keep the edit after closeout.
- IPC boundary reposition cannot overwrite comment-tail edits with stale disk content.
- The commit boundary for non-component late edits is explicit and tested.
