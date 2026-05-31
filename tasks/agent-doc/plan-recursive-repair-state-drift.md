# Plan: recursive owner-pane recovery should not leave document state wedged

**Pending:** `#recursive-repair-state-drift` (umbrella retired — done; remainder in discrete children)
**Status:** In progress via children

## Status note (2026-05-31) — slice 2: #recursive-repair-recovery-states LANDED

All four recovery states the plan enumerated are now classified in
`flow::closeout::classify_closeout_recovery_state`, each with one safe recovery
command (rendered by `session_check::closeout_recovery_hint`):

- `OpenEmptyPreflight` — empty `preflight_started` cycle (no capture / response
  hash / pending mutation) → `agent-doc cancel`. Distinguished from a real
  `OpenCycle` (which has a capture or pending mutation).
- `QueueMetadataDrift` — committed snapshot differs from HEAD only by queue /
  `queue_active` / status metadata (content-component signature identical) →
  `agent-doc commit`. Distinct from `BoundaryOnlyDrift` (boundary/prompt-prefix).
- `SidecarVisibleDrift` — snapshot matches HEAD but the visible file is stale by
  metadata only → `agent-doc reset --from-current --preserve-session` + commit.
- `UnsafeUserContentDrift` — real user/response content differs (content
  signature differs) → fail closed: preserve via `finalize`/`write --commit`,
  never auto-commit.

Discrimination uses a new `content_component_signature` (exchange + backlog +
review + icebox + done, excluding queue/status/frontmatter/boundary). Tests:
`classify_recovery_open_empty_preflight_when_nothing_followed`,
`classify_recovery_open_cycle_when_preflight_has_pending_mutations`,
`classify_recovery_queue_metadata_drift_when_only_queue_differs`,
`classify_recovery_unsafe_user_content_drift_when_exchange_differs`, plus the
expanded `recovery_command_maps_each_state_to_one_instruction`. These were
live-reproduced this session (empty probe cycle + queue-sync metadata drift
during the #gated-followup-split recovery churn).

Remaining children: `#recursive-repair-apply` (apply the safe states in one
command + live verify) and `#preflight-probe-side-effect-free`.

## Status note (2026-05-31) — slice 1: typed metadata-only HEAD-drift recovery

Proposed-Fix #3 partially landed by extending the existing
`flow::closeout::CloseoutRecoveryState` machine (the plan says extend, not
bypass, `#closeout-recovery-state-machine`):

- `classify_closeout_recovery_state` now wires the previously-reserved
  `BoundaryOnlyDrift` state. A committed cycle whose snapshot differs from HEAD
  **only** by agent-doc-generated exchange artifacts — transient boundary /
  `(HEAD)` markers and answered-prompt-prefix canonicalization — is classified
  `BoundaryOnlyDrift` and `session_check::closeout_recovery_hint` now names the
  single safe recovery `agent-doc commit <file>` instead of the generic
  `write --commit` chain. Detection: `committed_artifact_only_head_drift` proves
  no real content drift via `normalize_committed_exchange_artifacts` equality
  (fuller than `verify_snapshot_committed`'s transient-only normalization, which
  is what trips the snapshot-vs-HEAD guard on prompt-prefix drift). Conservative:
  any read error → `false`, so genuine content drift never masquerades as
  metadata-only. Test: `classify_recovery_boundary_only_drift_for_answered_prompt_prefix`.

**Remaining (gated):**
1. Recursive owner-pane recovery states the plan enumerates that this slice does
   *not* yet detect: `open_empty_preflight` (auto-abandon), `sidecar_visible_drift`
   (snapshot/CRDT/baseline stale vs visible after accepted metadata change),
   `unsafe_user_content_drift` (fail closed), and queue/`queue_active`-metadata
   HEAD drift (distinct from boundary/prompt-prefix; needs care so a user queue
   prompt edit is never auto-committed — non-goal).
2. Proposed-Fix #2: teach `agent-doc repair` to *apply* the safe states in order
   (abandon empty preflight, rebuild sidecars on metadata-only diff, commit
   agent-doc-generated metadata) so recovery is one command, not just one hint.
3. Proposed-Fix #4: make probe/diagnostic `preflight` side-effect-free or
   self-abandoning so it never leaves an open `preflight_started` cycle.
4. Live verification of the full recursive same-pane recovery path (needs a live
   owner pane; risky to exercise in a racy multi-session repo).

## Problem

Live repro from `src/boost-client/tasks/monsterrodholders.md` on 2026-05-31:

1. The document was owned by the current Codex pane and had an active auto queue.
2. A direct same-pane invocation of:
   `agent-doc /home/brian/work/btakita/agent-loop/src/boost-client/tasks/monsterrodholders.md`
   hit the recursive direct-invocation guard.
3. The guard abandoned one empty preflight cycle, but a follow-up `agent-doc
   preflight` probe opened another `preflight_started` cycle.
4. `session-check` then failed because the new cycle was still open.
5. After `agent-doc cancel`, the visible file and sidecars still needed manual
   convergence: `agent-doc reset --from-current --preserve-session`.
6. After sidecar convergence, `session-check` still failed because the visible
   document (`queue_active: true` and a regenerated boundary) did not match
   `HEAD`; the operator had to run `agent-doc commit`.

The document was recoverable, but only through an expert sequence:

```bash
agent-doc cancel tasks/monsterrodholders.md
agent-doc reset --from-current --preserve-session tasks/monsterrodholders.md
agent-doc commit tasks/monsterrodholders.md
```

That is too fragile for a normal Run Agent Doc recovery path. A recursive
same-pane guard should either leave no new mutable state behind or give one
deterministic repair command that closes all three invariants: cycle state,
sidecars, and repository HEAD.

## Desired Behavior

After a recursive direct-invocation block or a no-op diagnostic preflight:

- no empty `preflight_started` cycle remains open;
- snapshot, CRDT, and baseline sidecars match the visible document if the
  diagnostic path intentionally accepted visible queue/frontmatter changes;
- `session-check` does not fail with snapshot-vs-HEAD drift unless there is a
  real uncommitted response or user-authored prompt that must be preserved;
- the repair diagnostic names one safe command, not a multi-step manual recipe.

## Proposed Fix

1. Add a typed recovery classifier for recursive owner-pane diagnostics:
   - `clean_terminal`: cycle abandoned and no document/sidecar/HEAD drift;
   - `open_empty_preflight`: empty `preflight_started` cycle can be abandoned;
   - `sidecar_visible_drift`: snapshot/CRDT/baseline stale relative to visible
     file after an accepted metadata-only change;
   - `visible_head_metadata_drift`: visible file has metadata-only queue or
     boundary changes not committed to HEAD;
   - `unsafe_user_content_drift`: user-authored prompt/response text changed and
     requires explicit operator confirmation.
2. Teach `agent-doc repair` or a new closeout helper to apply the safe states in
   order:
   - abandon empty preflight cycles;
   - rebuild sidecars from visible content only when the diff is metadata-only;
   - commit the session document when the only HEAD drift is session metadata
     that agent-doc itself generated.
3. Make `session-check` emit the same typed recovery hint instead of repeatedly
   surfacing separate invariants.
4. Ensure `preflight` in probe/diagnostic contexts never opens a new cycle unless
   it is about to dispatch or write a response. If a probe must inspect current
   state, it should remain side-effect-free or auto-abandon before exit.
5. Integrate the recovery path with the broader
   `#codex-owned-pane-auto-queue-stuck` fix so owner-pane queue recursion does
   not create boundary-only or queue bookkeeping drift before handoff.

## Tests

1. Recursive same-pane block leaves no open cycle and `session-check` passes
   when no visible metadata changed.
2. A diagnostic `preflight` probe after the recursive block does not leave an
   open `preflight_started` cycle.
3. Metadata-only visible/sidecar drift is repaired by one command and does not
   require a manual reset.
4. Metadata-only visible/HEAD drift is either auto-committed by the repair path
   or reported with a single exact `agent-doc commit <file>` recovery.
5. User-authored exchange text drift is not auto-committed or hidden; the repair
   path fails closed with an explicit preservation diagnostic.

## Likely Files

- `src/agent-doc/agent-doc-orchestration/src/run.rs`
- `src/agent-doc/agent-doc-orchestration/src/preflight.rs`
- `src/agent-doc/agent-doc-orchestration/src/repair.rs`
- `src/agent-doc/agent-doc-orchestration/src/session_check.rs`
- `src/agent-doc/src/reset.rs`
- `src/agent-doc/tests/run_integration.rs`

## Related

- `#codex-owned-pane-auto-queue-stuck`: the broader owner-pane auto-queue handoff
  bug that caused this recovery path to matter.
- `#codex-owned-pane-prompt-miss`: same owner-pane recursion shape for exchange
  prompts.
- `#closeout-recovery-state-machine`: existing typed recovery work that this
  should extend rather than bypass.
- `#cancel-orphans-preflight-cycle`: prior empty-preflight cleanup for explicit
  cancel paths.

## Non-Goals

- Do not remove the recursive direct-invocation guard.
- Do not auto-commit user-authored prompts or response text.
- Do not hide real dirty working-tree state in sibling repos or submodules.
