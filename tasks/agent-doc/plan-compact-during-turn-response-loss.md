# Plan: response patchback loss when JB Compact Exchange runs mid-turn

**Bug ID:** `#adoc-compact-during-turn-response-loss`
**Class:** closeout / mid-turn external-cycle race
**Related:** `#adoc-jb-cache-cancel-stuck-cycle` (different mid-turn race, same wedge surface), `#adoc-prefix-strip-uncommitted` (downstream symptom on some variants), `#jbccfix`
**Severity:** high — every agent turn that races a JetBrains-initiated `agent-doc compact exchange` (or any binary-owned commit on the same doc) loses the assistant response with no clear automatic recovery beyond manual `git add` + `git commit`.

## Problem

A normal `agent-doc finalize <FILE>` cycle proceeds:

1. `agent-doc preflight <FILE>` — captures baseline, records cycle ID, returns the baseline file path.
2. Skill responds, pipes the response through `agent-doc finalize <FILE> --baseline-file <baseline>`.
3. `finalize` reads baseline, snapshots, merges patches, commits, runs `session-check`.

If, between steps 1 and 2, another agent-doc-owned commit lands on the same document — for example a JetBrains-triggered `agent-doc compact exchange` or any sibling write — the cycle state recorded at step 1 is invalidated:

- The pre-compact baseline no longer matches the post-compact snapshot.
- The cycle ID from step 1 is marked `committed` by the compact step's closeout.
- When step 2 (the skill's `finalize`) runs, the binary's late-fallback gate fires:
  ```
  [write] rejecting late fallback patch: cycle <ID> already committed for <FILE>
  ```
  followed by:
  ```
  [commit] detected post-commit local drift for <FILE> — HEAD already contains the committed response;
  leaving later local working-tree edits uncommitted
  [commit] staged snapshot already matches HEAD for <FILE> — closing cycle as already committed
  ```

The response body never reaches the document. The skill sees a non-error commit footer and may believe the cycle closed; only `session-check` (run separately) surfaces the divergence.

## Reproduction

Concrete instance from `agent-loop` (`tasks/agent-doc/agent-doc-bugs2.md`, 2026-05-25):

- `agent-doc preflight` at ~03:42:30 captured baseline for cycle `cycle-1779694939132`.
- Skill composed the `### Re: do [#jbccc2]` response and ran `agent-doc finalize ...`.
- At 03:42:55 a `compact exchange` ran (archive
  `.agent-doc/archives/<hash>-20260525-074255.md` + git commit `14bc483d3`).
- Skill's finalize hit `rejecting late fallback patch: cycle … already committed` and the response body was discarded.
- The gate / `--done` / `--pending-add` mutations applied earlier in the same `finalize` invocation HAD been written to the working tree (visible in the next preflight as a pending move), but the **response heredoc** never crossed the patch boundary.
- Recovery required `git add` + bare `git commit` of the unrelated working-tree mutation, then `agent-doc reset --from-current` to rebuild the snapshot.

## Root cause

`agent-doc finalize` re-reads the cycle-state file (`.agent-doc/cycle-state/<hash>.json`) before applying the response. When it sees the active cycle's phase is `Committed` and HEAD matches the snapshot, it concludes the response was already applied (via repair / dedupe path) and refuses to write the new body. The gate is correct for the *intended* case (the binary already absorbed the response). It is wrong for the *mid-turn compact* case (a different agent-doc operation closed the cycle without ever seeing the skill's response).

The binary has no signal that distinguishes these:

- "Cycle committed and snapshot==HEAD because the response is already in the doc" (skip).
- "Cycle committed and snapshot==HEAD because **a different** binary-owned operation committed unrelated work and reset the cycle phase" (need to start a *new* cycle and apply the response).

## Fix

Two coordinated changes; pick whichever ships first, the other becomes the second layer.

### Option A — `finalize` opens a fresh cycle when the captured baseline no longer matches HEAD

When `finalize` reads its CLI-provided `--baseline-file` and observes that the baseline content differs from the current snapshot / HEAD, treat this as "the cycle the baseline was captured for is no longer the active cycle." Specifically:

1. If `cycle-state.cycle_id` no longer matches the baseline-era cycle ID (or the cycle ID is missing because it was rotated by a mid-turn closeout), do not short-circuit on `already_committed`. Start a fresh cycle: re-run the preflight equivalent inline, capture a new baseline, then apply the response.
2. Emit a `mid_turn_baseline_drift` ops-log event so the divergence is auditable.

### Option B — Compact / any binary-owned commit on an active cycle leaves a "compact_landed_during_active_cycle" marker

When `compact exchange` (or any other binary-owned commit that lands while another cycle's `cycle-state` is `PreflightStarted` / `ResponseCaptured`) closes its own cycle, write a sidecar marker (`.agent-doc/cycle-state/<hash>.compact-landed.json`) noting which cycle ID was active at the time. The next `finalize` reads the marker and treats its baseline as "stale; start a new cycle without short-circuiting."

Option A is the cleaner fix because it generalizes to any mid-turn binary-owned commit (not just compact). Option B is narrower but easier to test in isolation.

## Scope

- `src/agent-doc/src/write.rs` — `finalize` / `complete_required_closeout` paths and the `late_fallback_patch` gate.
- `src/agent-doc/src/cycle_state.rs` — read/compare baseline against post-commit cycle-state.
- `src/agent-doc/src/compact.rs` — if Option B, write the sidecar marker.
- `src/agent-doc/src/preflight.rs` — emit a `mid_turn_baseline_drift` warning when the snapshot has rotated since the baseline was captured but no fresh cycle has started.
- New SimWorld scenario in `src/agent-doc/src/sim_world.rs` modeling the mid-turn-compact race (similar to `#jbccc1`).

## Out of scope

- Preventing JetBrains from initiating `compact exchange` while another cycle is in flight. The IDE can't always know a terminal cycle is active.
- Changing the binary's cycle-state file format. The fix should add a new field, not break readers.

## Verification

1. SimWorld scenario: capture baseline → another `compact exchange` cycle commits → original `finalize` runs → assert the response lands in the new cycle without manual recovery.
2. Manual repro: open a session doc in IntelliJ + open this terminal; run `/agent-doc <FILE>` to start a cycle; trigger JB Compact Exchange via the plugin during the same turn; assert the next `finalize` lands the response.
3. `make check` clean.
4. Add an ops-log assertion that `mid_turn_baseline_drift` events fire when the race condition is detected.

## Tracking

- Surfaced from: `tasks/agent-doc/agent-doc-bugs2.md` cycle attempting `#jbccc2` gate, 2026-05-25 03:42–03:45 EDT.
- Backlog home: `tasks/agent-doc/agent-doc-bugs2.md` — id `#adoc-compact-during-turn-response-loss`.
