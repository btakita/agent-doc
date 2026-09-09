# Preserve unanswered exchange prompts during recovery

Invariant: recovery without a new answer leaves the operator's exchange prompt
uncommitted and available to the next preflight.

Policy owner: commit-io's owned-component commit planner and guarded snapshot
adoption; preflight-io consumes the existing typed repair outcome.

Transition table:
- No retained component changes (empty or no-op journal): guarded whole-document fallback.
- Unowned exchange changed: guarded whole-document fallback, preserving unanswered prompts.
- Owned target delivered with only unrelated operator component changes: rebase.
- Owned target not delivered: block.
- Legacy/malformed journal: existing guarded fallback.
- New captured answer materialized: existing response closeout.
- Previously committed answer plus fresh prompt: do not absorb prompt.
- Stale empty cycle abandoned: skip the recovery commit; admit a fresh prompt cycle.

Evidence inputs: retained expected/target transitions, current authoritative text,
snapshot, HEAD response materialization, typed RepairOutcome.

Reactive topology: existing document-scoped preflight Sources → readiness
Computed → repair/commit Effects → receipt Sources remain the lifecycle owner.
The commit planner operates on a single immutable transaction cut. The changed
decisions have no persistent cache or independent retry loop; a new graph would
be a disconnected island. No new orchestration or polling is introduced.

Imperative extraction audit: repair currently discards its typed abandonment
outcome before invoking commit. Preserve that outcome at the effect boundary.
Snapshot planning and HEAD membership are pure one-shot transaction decisions,
so they remain in their existing owner rather than creating per-call Computeds.

Allowed edit surfaces: commit-io planner/adoption and tests; preflight-io recovery
adapter and preflight-command-io integration tests; release projection and notes.

Verification: empty/no-op and unowned-exchange planner tests; committed-response
and no-capture git regressions with the experimental flag both off/on; repeated
stale-preflight recovery preserves HEAD, disk prompt and actionable diff;
full make check, latest CI inspection, release install verification.

Out of scope: changing Codex itself, queueing duplicate prompts, editing the
already answered fpe document, or changing component ownership semantics.

## Verification result

`make check` passed: 9,262 tests, 256 skipped, plus simulator, documentation,
editor parity, Lean and TLA checks. Regression coverage includes the feature
flag off/on, absent/committed captures, prefixed/plain fenced prompts, and three
consecutive recovery attempts. Latest CI before this change was green.

The initial broad in-process Cargo test run was stopped after its commit tests
passed and preflight tests stalled; the full standard nextest run above passed
all tests in isolated processes with the repository's timeout policy.

## Reused-heading edge found during final review

A negative regression (`older follow-up question` after `Re: older`) proved that
post-commit cleanup could delete a fresh question even without a capture.
Expand the policy owner/edit surface to document/write_normalization: cleanup
requires exact committed queue-prompt proof, never a similar response heading.
Remove its fuzzy heading matcher rather than add another exception. An exact
queue echo with a missing committed proof may reconcile; retained proofs plus
a new prompt are ambiguous and must stay visible. Batch wording without exact
proof likewise stays visible. Commit-io must exclude already committed captures
from both prompt matching and adoption authorization.

These are pure decisions over the same immutable transaction cut; no new
lifecycle, cached readiness, polling, or reactive graph is introduced. Extend
the git matrix and pure cleanup tests, then rerun the complete verification.
The 0.35.351 installation was stopped before replacing the installed 0.35.350
binary; the follow-up release includes this additional preservation fix.

The reused-heading regression failed before the normalization fix and passed
afterward. All 9 focused checks passed. The expanded full run passed 9,263 tests
(256 skipped); `make tmux-ci` passed its 8 general and 13 layout-repair tests.
CI initially failed before tests on a Google apt-index hash mismatch; its
requested retry passed package installation and continued normally.

Final `make check` completed successfully, including documentation, both editor
test suites, native cross-editor simulation, Lean and TLA. Final source commit:
`c7b3023ed` (0.35.352), following the initial `0453a3282` preservation fix.
