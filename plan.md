Moved to [tasks/plan.md](tasks/plan.md).

- [x] Inspect `tasks/software/agent-loop.md` and `src/agent-doc/CLAUDE.md` to identify the concrete requested transfer.
- [x] Inspect `tasks/agent-doc/agent-doc-feat.md` to find the correct insertion points for exchanged context and pending items.
- [x] Transfer the snapshot-GC feature context into `tasks/agent-doc/agent-doc-feat.md` with a concrete, configurable-reap-cycle spec and pending item.
- [x] Update `tasks/software/agent-loop.md` to mark the transfer item complete and record the transfer in the exchange.
- [x] Review the diffs for only the touched files.

## 2026-04-23 — `agent-doc-bugs` `#cshk`

- [x] Confirm the current Codex hook contract and decide the minimal binary-owned integration surface for `#cshk`.
- [x] Implement the Codex hook state/capture flow in `src/agent-doc`, keeping deterministic logic in Rust rather than in generated hook scripts.
- [x] Install the Codex hook/config artifacts from `agent-doc skill install`, add regression coverage, then run the requested verification (`cargo test`, `make check`, `cargo build --release`, `cargo install --path .`).
- [x] Patch the result back into `tasks/agent-doc/agent-doc-bugs.md` and commit the required repo changes in the correct repo boundaries.

## 2026-04-23 — `agent-doc-bugs` failing tests follow-up

- [x] Reproduce the current `src/agent-doc` failures for the open `#cyc1` follow-up and isolate whether the implementation or test expectations are wrong.
- [x] Patch the minimal `src/agent-doc` fix, keeping docs/specs aligned if behavior changed.
- [x] Re-run the relevant verification, patch the outcome into `tasks/agent-doc/agent-doc-bugs.md`, and commit the required repo changes.

## 2026-04-23 — `agent-doc-bugs` `#wcmp`

- [x] Inspect the current `src/agent-doc` skill, runbooks, specs, and tests to find any manual-repair guidance that still stops at bare `agent-doc write`.
- [x] Update the shared/manual-repair guidance so both Codex and Claude default to `agent-doc write --commit <FILE>` and keep installed-content parity intact.
- [x] Add or adjust regression coverage for the new guidance, then run build/install verification and patch the result back into `tasks/agent-doc/agent-doc-bugs.md`.

## 2026-04-23 — `agent-doc-bugs` `#cmiv`

- [x] Inspect the current commit/write guidance in `src/agent-doc` to decide whether `#cmiv` should land as a new `runbooks/commit.md`, a skill rewrite, or both, while avoiding overlap with the existing `finalize` worktree changes.
- [x] Update the minimal source docs/runbooks so the hard invariant is explicit: every appended `agent-doc` response must cross a commit boundary unless the user explicitly says otherwise, with Claude/Codex-specific invocation differences still delegated to `harness-invocation.md`.
- [x] Add regression coverage in `src/agent-doc/src/skill.rs` (and any other necessary tests) to lock installed-content parity for the new commit invariant.
- [x] Run the requested verification in `src/agent-doc` (`cargo test`, `make check`, `cargo build --release`, `cargo install --path .`) and only then patch the outcome back into `tasks/agent-doc/agent-doc-bugs.md`.

## 2026-04-23 — `agent-doc-bugs` `#cyc1` follow-up

- [x] Reproduce the current `src/agent-doc` failures around the open cycle-completion work and identify whether the breakage is in the implementation or the tests.
- [x] Patch the minimal `src/agent-doc` fix, keeping docs/specs/tests aligned with the enforced cycle contract.
- [x] Re-run the relevant verification, patch the result back into `tasks/agent-doc/agent-doc-bugs.md`, and commit the required repo changes in the correct repo boundaries.
