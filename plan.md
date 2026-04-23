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

## 2026-04-23 — `agent-doc-bugs` Codex follow-up verification

- [x] Inspect the unfinished `src/agent-doc` changes and identify the failing tests or checks tied to the Codex follow-up.
- [x] Decide whether the right action is a code/test fix or verification-only closeout based on the live worktree.
- [x] Verify the current `src/agent-doc` tree with targeted Codex tests and `make -C src/agent-doc check`.
- [x] Update `tasks/agent-doc/agent-doc-bugs.md` with the outcome.

## 2026-04-23 — `agent-doc-bugs` Codex patchback not committed analysis

- [x] Compare the committed vs working-tree state for `tasks/agent-doc/agent-doc-bugs.md` to identify exactly which response patchback never crossed a parent commit boundary.
- [x] Verify the live `agent-doc` guard state for this workspace (`session-check`, cycle-state file, and installed `.codex` hook/config artifacts) to determine whether the failure came from a bypassed write path, missing hook installation, or both.
- [x] Patch the evidence-backed analysis into `tasks/agent-doc/agent-doc-bugs.md` without disturbing unrelated root or submodule worktree changes.

## 2026-04-23 — `agent-doc-bugs` Codex deterministic patchback closeout

- [x] Inspect the current Codex stop-hook and response-persistence boundary to decide the minimal deterministic closeout path when Codex reaches `Stop` with an open `agent-doc` cycle.
- [x] Implement the binary-owned fix in `src/agent-doc`, keeping Codex/manual-repair docs and tests aligned with the enforced ordering.
- [x] Run focused verification first, then the requested broader build/install checks if the code change lands cleanly.
- [ ] Patch the result back into `tasks/agent-doc/agent-doc-bugs.md`.

## 2026-04-23 — `agent-doc-bugs` harness-priority next steps

- [x] Inspect the current `tasks/agent-doc/agent-doc-bugs.md` ask plus the unfinished `src/agent-doc` worktree to separate already-in-flight closeout fixes from still-missing workspace/runtime steps.
- [ ] Verify the current Codex closeout WIP in `src/agent-doc` with focused hook/session tests and `make -C src/agent-doc check`, so the next action is based on the live tree rather than prior turn assumptions.
- [ ] Install the updated `agent-doc` skill/artifacts into this `agent-loop` workspace and confirm the repo-local `.codex/hooks.json` and `.codex/config.toml` backstop are actually present here.
- [ ] Run an end-to-end Codex closeout check in `agent-loop` that proves both the normal `finalize` + `session-check` path and the `Stop` hook backstop leave no uncommitted patchback.
- [ ] Patch the verified outcome back into `tasks/agent-doc/agent-doc-bugs.md`, update pending state, and only then commit/push in the correct repo boundaries.

## 2026-04-23 — `agent-doc-bugs` `#bndy`

- [x] Re-read the current boundary cleanup contract in `src/agent-doc` and reproduce the targeted `git.rs` boundary tests so the task is grounded in the live tree.
- [x] Patch the minimal `src/agent-doc` code/spec/test surface needed to prove post-commit boundary cleanup stays clean with both a live IPC listener and a stale `.agent-doc/patches/` directory.
- [x] Run the requested verification in `src/agent-doc` (`cargo test`, `make check`, `cargo build --release`, `cargo install --path .`), then patch the verified outcome back into `tasks/agent-doc/agent-doc-bugs.md`.

## 2026-04-23 — `agent-doc-bugs` `#ha3h`

- [x] Re-read the open bug item plus the linked `agent-doc` issue to confirm the intended recipient-side config shape and any explicit ship gate.
- [x] Verify from the live `src/agent-doc` tree whether multi-user workspace support is already present or still explicitly absent.
- [ ] If the prerequisite is present, implement the shared recipient-policy path for pending auto-create in `write`/`notify`, update specs/tests, and run the relevant verification.
- [x] If the prerequisite is absent, patch `tasks/agent-doc/agent-doc-bugs.md` with the evidence-backed deferral/blocker instead of shipping the config early.

## 2026-04-23 — `agent-doc-bugs` boost-client `monsterrodholders` patchback analysis

- [x] Inspect the committed vs working-tree state for `src/boost-client/tasks/monsterrodholders.md` to see whether the `#h6gx` patchback ever crossed a commit boundary.
- [x] Compare the recent `src/boost-client` commits with the `.agent-doc` snapshot / cycle-state / ops-log evidence to determine whether the patchback was skipped, reverted, or left as out-of-band drift.
- [x] Patch the evidence-backed answer back into `tasks/agent-doc/agent-doc-bugs.md` without changing `src/agent-doc`.

## 2026-04-23 — `agent-doc-bugs` snapshot-drift recovery question

- [x] Inspect the current `src/agent-doc` selective-commit, recovery, and session-check paths to determine what already happens when the live file drifts from the snapshot.
- [x] Decide whether the right fix is new recovery logic, stricter fail-closed behavior, or just documentation based on the existing product contract and the `monsterrodholders` evidence.
- [x] Patch the conclusion back into `tasks/agent-doc/agent-doc-bugs.md`, and implement/test in `src/agent-doc` only if the investigation shows a contract gap rather than expected fail-closed behavior.

## 2026-04-23 — `agent-doc-bugs` snapshot-drift closeout

- [x] Run the remaining requested local verification for the new `preflight_started` fail-closed guard in `src/agent-doc` (`cargo build --release`, `cargo install --path .`).
- [x] Patch the verified build/install + commit/push outcome back into `tasks/agent-doc/agent-doc-bugs.md`.
- [x] Commit and push the `src/agent-doc` changes on the current branch, then update the parent repo submodule pointer / task docs in a separate `agent-loop` commit.
