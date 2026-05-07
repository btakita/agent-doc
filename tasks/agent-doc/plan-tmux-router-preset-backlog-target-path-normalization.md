# Plan — normalize repo-local backlog target paths in session prompt presets

## Problem

The `tasks/software/tmux-router.md` frontmatter introduced an `#agent-doc-bug`
preset whose backlog target was written as
`agent-loop/tasks/agent-doc/agent-doc-bugs2.md` from inside the `agent-loop`
repository root. During closeout, `agent-doc finalize` resolved that as a
doubled path under the current repo and failed the required backlog-capture
guard even though the intended target document existed and was updated.

## Goal

Make repo-local backlog targets resolve deterministically when users copy a
project-qualified path into a session document that already lives inside that
same repo.

## Implementation outcome

- [x] Reproduce the doubled-path failure with a minimal session doc that uses an
  `#agent-doc-bug` preset pointing at `agent-loop/tasks/...` from the repo root.
- [x] Decide whether prompt-contract path normalization should strip a redundant
  leading repo directory name or fail earlier with a clearer validation error.
- [x] Add regression coverage for whichever normalization or validation rule is
  chosen.

Resolution: strip a redundant leading project-root directory name before falling
back to the unresolved root-joined path. This keeps copied project-qualified
paths working without weakening the existing cross-document review gate.

## Exit criteria

- `finalize` and `session-check` resolve repo-local backlog targets to the
  intended document or emit a deterministic preflight error before closeout.
