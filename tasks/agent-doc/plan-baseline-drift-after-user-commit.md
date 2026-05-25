# Plan: baseline drift after a manual user commit on top of an agent-doc commit

**Bug ID:** `#adoc-baseline-drift-after-user-commit`
**Bug class:** repair / capture-replay
**Related:** `#adoc-prefix-strip-uncommitted` (downstream surface — manual `sed` strip is one trigger), `#adoc-jb-cache-cancel-stuck-cycle` (Phase 3 #jbccc3 closes the IPC-cancel variant; user-driven manual commits are the still-uncovered case).
**Severity:** medium — every cycle where the user lands their own `git commit` on top of an agent-doc commit before the next preflight wedges `preflight` / `write --commit` / `repair` until the operator runs `agent-doc reset --from-current`, which drops session ID + snapshot history.

## Problem

`capture::validate_replay` (`src/capture.rs:437`) compares the captured `file_hash` and `snapshot_hash` against the live document and snapshot via exact SHA-256 equality. Any byte-level change to the document — including a benign manual user commit that adds new prompts, edits backlog items, or rewords prose outside the response body — flips the file hash and trips the gate:

```
captured response baseline no longer matches current document for <FILE>
```

The same gate fires from `preflight::run` → `repair::run` → `validate_replay` (`src/repair.rs:1396`) and from `write --commit` repair flows, so every downstream path inherits the failure. The only documented recovery is `agent-doc reset --from-current`, which rebuilds the baseline from the live markdown but destroys session-id continuity and the snapshot history that `agent-doc undo` and `agent-doc history` depend on.

The captured baseline at `.agent-doc/baselines/<hash>.md` is itself a stable post-commit copy — the bug is not that the file is missing; it is that the equality check is too strict for the common "user committed something benign in between" workflow.

## Repro

1. Run a normal agent-doc cycle to completion (`agent-doc finalize <FILE>` clean).
2. The user manually edits the document (add a new backlog item, fix a typo in the exchange, etc.) and commits it directly with `git commit`, bypassing the binary-owned closeout.
3. Run `agent-doc preflight <FILE>` (or any flow that calls `repair::run`).
4. Observed: `Error: captured response baseline no longer matches current document for <FILE>`. `write --commit` / `session-check` / `finalize` all share the failure.
5. Expected: preflight either refreshes the baseline silently (when the drift is benign and outside the captured response body) or surfaces a tailored "manual commit detected — re-baseline?" message with a non-destructive recovery command.

A deterministic SimWorld scenario should pin the three branches: (a) drift inside the captured response body — must fail closed; (b) drift outside the response body — auto-refresh baseline and proceed; (c) drift inside the response body matches normalized response — adopt and proceed.

## Root-cause hypotheses

1. **The hash gate is too coarse.** `validate_replay` compares whole-file SHA-256 hashes. The capture record stores `file_hash` / `snapshot_hash` but no structural fingerprint, so even purely additive non-overlapping changes invalidate replay.
2. **`agent-doc reset --from-current` is the only sanctioned recovery** but it nukes `.agent-doc/state/cycles/<hash>.json` (session id), snapshot history, and capture history. Users who hit this twice in a session lose `agent-doc undo` and `agent-doc history` continuity.
3. **No detection path classifies "user committed a manual edit"** distinct from genuine corruption / out-of-band patchback, so the binary cannot pick a targeted recovery.

## Plan

### Phase 1 — Deterministic SimWorld repro

**File:** `src/sim_world.rs`

1. Add a scenario covering the three branches above.
2. Three test cases, mirroring the JB cache-conflict pattern:
   - `baseline_drift_benign_user_commit_outside_response_auto_refreshes` (target: must pass after Phase 2).
   - `baseline_drift_user_edit_inside_committed_response_fails_closed` (target: must always fail closed — invariant).
   - `baseline_drift_user_edit_matches_normalized_response_adopts` (target: adopts the user-cleaned response as the cycle's response — closes the `#adoc-prefix-strip-uncommitted` overlap).

### Phase 2 — Binary: classify benign user commits and auto-refresh

**Files:** `src/capture.rs`, `src/repair.rs`, `src/preflight.rs`

1. In `validate_replay`, when the file/snapshot hash mismatches but `HEAD == capture.file_hash` (the capture is still the prior committed state) and the diff between captured baseline and the current file is **outside the captured response body** and **does not overlap any tracked component the cycle was active for**, treat it as `benign_user_commit_drift` and refresh `.agent-doc/baselines/<hash>.md` + the capture's `file_hash` / `snapshot_hash` from the current state.
2. When the drift overlaps the response body but the working tree's normalized response matches the captured one, treat as `adopted_user_normalized_response` and refresh the baseline without breaking the cycle (this closes the manual `sed` strip case overlap with `#adoc-prefix-strip-uncommitted`).
3. For any other drift, keep the existing fail-closed behavior but emit the tailored hint added by Phase 3 instead of the current opaque message.

### Phase 3 — Tailored recovery command

**Files:** `src/reset.rs`, `src/capture.rs`, `src/skill.rs`

1. Add `agent-doc reset --from-current --preserve-session` that rebuilds the baseline from the live document but keeps `.agent-doc/state/cycles/<hash>.json`, the snapshot history, and the active capture chain.
2. When auto-refresh refuses (Phase 2 case 3), the bail message should print the exact `agent-doc reset --from-current --preserve-session <FILE>` command so operators don't have to consult docs.
3. Skip the destructive reset path unless the user explicitly opts in (existing `--from-current` retains current destructive behavior for parity).

### Phase 4 — Documentation

**Files:** `runbooks/commit.md`, new `runbooks/baseline-drift.md` (or extend `jb-cache-conflict.md` cross-link), `SKILL.md`

1. Document the auto-refresh contract (benign drift outside response body).
2. Document the new `--preserve-session` recovery command.
3. Cross-link from `runbooks/commit.md` and the SKILL.md catalog.

### Phase 5 — Regression coverage

1. SimWorld Phase 1 scenarios must pass after Phase 2.
2. In-source preflight integration test (sibling of `preflight_auto_recovers_jb_cache_conflict_cancel_committed_with_snapshot_drift`): set up a real git repo, simulate the benign manual-commit drift, run preflight, assert the baseline refreshes and the cycle proceeds without operator intervention.
3. Negative test: drift inside the captured response body still bails with the tailored recovery hint.

## Testing

- `cargo test baseline_drift` after each phase.
- Manual repro: finish an agent-doc cycle, manually `git commit --amend` or stack a new commit with an unrelated edit, then `agent-doc preflight <FILE>`. Should pass cleanly.
- `make check` clean.
