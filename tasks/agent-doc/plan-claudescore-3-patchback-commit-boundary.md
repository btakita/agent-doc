# Plan — `claudescore-3.md` patchback bypassed the commit boundary

- [ ] Document the exact failure chain for `src/session-share/tasks/claudescore-3.md`: the session promised an in-document report-back, the response was later patched directly into the document, and no `agent-doc` `write --commit` or `finalize` cycle ran for that write.
- [ ] Include the current `2026-04-27` reproduction shape in that failure chain: the worktree shows a visible `❯ ...` + `### Re:` exchange block for the UTC-news response, `src/session-share` can be manually committed afterward (`9be7d48`), but `agent-doc session-check` still interrupts with `found likely direct response patchback without agent-doc cycle`, so the detector/repair path is not recognizing this apparently well-formed, committed `claudescore-3.md` exchange as a recoverable closeout state.
- [ ] Inspect the active `agent-doc` closeout surfaces in `src/agent-doc` (`route`, `plan`, `write`, `finalize`, `preflight`, `session-check`, and repair/Stop recovery) to identify where a session-document run can appear complete without a committed document update.
- [ ] Define a binary-owned invariant for session documents: if the target document receives a substantive response patchback, the cycle is only clean after a commit acknowledgment exists for the owning repo or submodule.
- [ ] Design submodule-aware commit ownership so document patchbacks commit in the repo that owns the document even when the parent worktree is dirty, while still surfacing that the parent pointer commit may remain intentionally deferred.
- [ ] Add explicit detection for the bad state: active target document changed, response content present, but no matching `commit_*` event or git commit acknowledgment for that patchback.
- [ ] Implement deterministic recovery so `preflight`, `session-check`, or repair can resume the missing commit step when hashes prove safety, or fail closed with a specific phase-labeled error when they do not.
- [ ] Ensure backlog/pending/status mutations in a patchback cannot bypass typed `agent-doc` operations simply because the response reached the document through an out-of-band edit path.
- [ ] Add regression coverage for:
  - committed session-document patchback in a normal repo
  - committed patchback in a submodule with a dirty parent repo
  - write-landed / commit-missing recovery
  - out-of-band document edit detection blocking a false "clean" completion
- [ ] Update the relevant spec and runbook surfaces only after the code-level invariant and recovery behavior are implemented and tested.
- [ ] Run targeted verification in `src/agent-doc`, then patch the outcome back into the tracking document and reduce the backlog to the remaining unresolved state classes.
