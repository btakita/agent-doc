# Plan — `agent-doc-bugs` response-not-committed delay

- [x] Inspect the `agent-doc` response persistence path (`finalize`, `write --commit`, `session-check`, and any post-write verification/install steps) to identify where the live response becomes visible before the commit boundary completes.
- [x] Reproduce or at least trace the delay path in `src/agent-doc` closely enough to distinguish expected post-write verification work from an actual missed-commit/runtime-ordering bug.
- [x] Implement the minimal fix in `src/agent-doc` if the current behavior is wrong; otherwise tighten the docs/spec/instruction surface so the visible ordering and commit semantics are explicit and testable.
- [x] Add regression coverage for the identified ordering/commit behavior.
- [x] Run the relevant verification in `src/agent-doc`, then patch the result back into `tasks/agent-doc/agent-doc-bugs.md` and update pending if new follow-up work remains.
