# Plan — preserve prompt prefixes on already-answered prompts after closeout

**Pending:** `#qprx` in `tasks/agent-doc/agent-doc-bugs2.md`
**Status:** Draft

## Problem

`tasks/agent-doc/agent-doc-bugs2.md` currently reproduces a closeout regression where an already-answered user prompt loses the binary-owned `❯ ` prefix in the live working tree even though the turn already reported success.

Reference repro from Apr 30, 2026:

- committed exchange line: `❯ commit + push all uncommitted src/agent-doc files`
- live working tree line after closeout: `commit + push all uncommitted src/agent-doc files`
- same diff also shows a `(HEAD)` suffix added to the response heading and a fresh boundary-id change

This is narrower than the older duplicate-response bug. The response body already exists and is not duplicated; the regression is that some closeout/adoption path mutates an already-answered prompt line after the response has landed, leaving residual dirt that looks like a new user edit.

## Goal

Identify which binary-owned closeout path stripped the `❯ ` prefix from an already-answered prompt, then make every equivalent path preserve prompt canonicalization or fail closed before reporting success.

## Desired Behavior

- once a prompt line is already answered by a real `### Re:` block, binary-owned closeout paths must not remove its `❯ ` prefix in the working tree
- a reported-success turn must not leave behind prompt-prefix-only drift on historical exchange lines
- `(HEAD)` marker cleanup, boundary cleanup, response adoption, and replay repair must agree on the same prompt-prefix invariant
- if a repair/adopt-current path cannot prove the canonical transcript shape, it must fail closed instead of rewriting the prompt line

## Questions To Answer First

- Which path produced the current `agent-doc-bugs2.md` diff:
  - normal `finalize`
  - `write --commit` replay
  - `repair::AlreadyApplied` adoption
  - stop-hook/backstop completion
  - boundary cleanup after commit
- Did the prefix disappear in the committed blob, in the snapshot, or only in a later working-tree rewrite?
- Why did the same path also leave a `(HEAD)` suffix and boundary-id drift:
  - one shared "current transcript adoption" rewrite
  - separate post-commit normalizers fighting each other
- Is the mutating logic reusing the historical answered-prompt prefix stripping helper in the wrong direction?

## Implementation Steps

- [ ] Reconstruct the exact closeout chain for `tasks/agent-doc/agent-doc-bugs2.md` from local evidence:
  - current working-tree diff
  - committed `HEAD` shape
  - snapshot/baseline state if present
  - any ops/session logs that show whether finalize, repair, or hook replay ran last
- [ ] Trace every path that rewrites exchange content after a response already exists:
  - final transcript normalization
  - `AlreadyApplied` / adopted-current repair
  - committed historical snapshot self-heal
  - boundary cleanup / `(HEAD)` cleanup
- [ ] Prove whether any helper intentionally strips prompt prefixes on already-answered lines and ensure that logic is only used for comparison/repair, never for the live visible transcript
- [ ] Tighten the canonical transcript invariant:
  - visible exchange content keeps `❯ ` on prompt-target lines
  - comparison-only normalizers may strip prefixes internally but must not write that stripped form back to the live doc
- [ ] Add regression coverage for the exact Apr 30 reproduction:
  - committed prompt line already has `❯ `
  - closeout/adoption path runs
  - resulting working tree must keep the prefix and must not add fresh `(HEAD)`/boundary drift on the answered prompt block
- [ ] Add a failure-path test:
  - if the path cannot preserve canonical prompt prefixes, it exits nonzero / keeps the cycle open instead of reporting success

## Files Likely To Change

- `src/agent-doc/src/write.rs`
- `src/agent-doc/src/repair.rs`
- `src/agent-doc/src/session_check.rs`
- `src/agent-doc/src/git.rs`
- `src/agent-doc/src/template.rs`
- `src/agent-doc/specs/07-commands.md`

## Exit Criteria

- the `agent-doc-bugs2.md` reproduction no longer strips `❯ ` from the historical prompt line
- reported-success closeout paths do not leave prompt-prefix-only drift in the working tree
- tests cover finalize/replay/adoption paths for already-answered prompt-prefix preservation
