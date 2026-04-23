Moved to [tasks/plan.md](tasks/plan.md).

- [x] Inspect `tasks/software/agent-loop.md` and `src/agent-doc/CLAUDE.md` to identify the concrete requested transfer.
- [x] Inspect `tasks/agent-doc/agent-doc-feat.md` to find the correct insertion points for exchanged context and pending items.
- [x] Transfer the snapshot-GC feature context into `tasks/agent-doc/agent-doc-feat.md` with a concrete, configurable-reap-cycle spec and pending item.
- [x] Update `tasks/software/agent-loop.md` to mark the transfer item complete and record the transfer in the exchange.
- [x] Review the diffs for only the touched files.

## 2026-04-23 — `agent-doc-bugs` failing tests follow-up

- [x] Reproduce the current `src/agent-doc` failures for the open `#cyc1` follow-up and isolate whether the implementation or test expectations are wrong.
- [x] Patch the minimal `src/agent-doc` fix, keeping docs/specs aligned if behavior changed.
- [x] Re-run the relevant verification, patch the outcome into `tasks/agent-doc/agent-doc-bugs.md`, and commit the required repo changes.

## 2026-04-23 — `agent-doc-bugs` `#wcmp`

- [x] Inspect the current `src/agent-doc` skill, runbooks, specs, and tests to find any manual-repair guidance that still stops at bare `agent-doc write`.
- [x] Update the shared/manual-repair guidance so both Codex and Claude default to `agent-doc write --commit <FILE>` and keep installed-content parity intact.
- [x] Add or adjust regression coverage for the new guidance, then run build/install verification and patch the result back into `tasks/agent-doc/agent-doc-bugs.md`.
