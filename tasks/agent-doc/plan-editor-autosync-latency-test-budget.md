# Plan - editor autosync, sync latency, and tmux test budget

**Pending:** `#autosync`, `#synclag`, `#tmuxbudget`
**Status:** Implemented through Phase 3

## Problem

The latest protected-layout fix made manual `Sync Tmux Layout` able to bring the
requested document pane into view, but the editor-driven path still has two
operator-visible gaps:

- changing the selected editor file does not reliably move focus or visible
  panes in the agent-doc tmux window
- manual `Sync Tmux Layout` can take several seconds after the project
  controller was introduced

The current regression strategy also leans heavily on real tmux integration
tests for edge cases that are increasingly expressible in deterministic
controller and SimWorld schedules.

## Goals

- Make editor file selection changes apply the same pane/focus result as manual
  sync when the target pane is safe to show.
- Keep normal safe-passive editor sync on a snappy path with phase-level latency
  diagnostics for controller lookup, ownership proof, tmux-router reconcile, and
  deferred retry waits.
- Reduce the long-term need for slow tmux-backed edge-case regressions by moving
  ownership and controller schedule coverage into deterministic simulator tests.

## Non-Goals

- Do not weaken protected closeout-cycle guards to make sync appear faster.
- Do not let editor autosync autostart new sessions unless the CLI safe-passive
  contract explicitly permits it.
- Do not delete existing tmux tests until an equivalent faster test covers the
  same failure mode and at least one real tmux smoke test still exercises the
  end-to-end path.

## Phase 1 - fix editor selection autosync correctness

Root the first implementation in the JetBrains and VS Code automatic sync paths,
not in manual `Sync Tmux Layout`.

1. Add a live reproduction trace for editor navigation:
   - current selected file
   - visible markdown layout
   - generated `agent-doc focus` or `agent-doc sync --no-autostart` command
   - CLI output
   - whether tmux selected the requested pane
2. Compare automatic sync against manual sync for the same layout.
   - If manual sync succeeds but automatic sync does not, inspect editor-side
     dedup state, generation locking, deferred retry handling, and focus-vs-sync
     planning.
   - If both use the same CLI path but only manual sync applies, inspect CLI
     safe-passive handling and whether `focus` lacks the repair/reselect behavior
     now present in sync.
3. Add editor plugin tests for:
   - selection changes that keep the visible set stable but require focus change
   - automatic `sync --no-autostart` output that preserves layout but reselects
     focus and must count as applied
   - queued latest selection replay after a slow sync finishes
4. Add one Rust regression for the CLI-level focus handoff if the failure lands
   below the plugin layer.

Acceptance criteria:

- switching between two visible agent-doc files updates tmux focus without
  requiring manual `Sync Tmux Layout`
- switching to a stashed but safe-to-show document brings the pane visible on the
  passive path
- editor logs distinguish "dedup skipped", "queued behind running sync",
  "deferred protected layout", and "applied"

## Phase 2 - make sync latency accountable

The controller architecture is allowed to add authority, but not unexplained
seconds of delay on the normal editor path.

1. Capture an actual slow manual sync run and extract all `sync_latency` and
   controller IPC log phases from `.agent-doc/logs/ops.log`.
2. Split any broad timing bucket that still hides controller launch/connect,
   authoritative actor lookup, projection refresh, tmux-router reconcile, and
   deferred retry delay.
3. Keep controller startup/version invalidation off the common path:
   - reuse a live same-binary controller
   - restart stale binary-generation controllers explicitly
   - bound connect/read/write attempts so a stalled client cannot serialize the
     next sync
4. Add a focused latency-budget test around the pure timing formatter and, where
   practical, an integration test that asserts a no-op safe-passive sync does
   not wait on dispatch proof or lifecycle ack phases.

Acceptance criteria:

- a slow sync report names the exact over-budget phase
- no-op safe-passive sync stays under the documented one-second budget in a
  warm-controller case
- controller cold-start cost is visible and not confused with tmux-router work

## Phase 3 - retire redundant slow tmux edge tests behind simulator coverage

The answer to "do we still need so many slow tmux integration tests?" should be
"no, not for pure ownership schedules." Keep real tmux tests for shell, pane,
window, and process behavior; move generated state combinations into SimWorld.

1. Inventory tmux-backed tests by purpose:
   - real tmux mechanics: keep as smoke or integration coverage
   - ownership generation, projection drift, stale pane, missing pane,
     dispatch-proof, and closeout-cycle guards: candidate for SimWorld
   - historical bug repros that combine both: split into one fast model test plus
     one smaller tmux smoke
2. Extend `SimWorld` route/controller schedules only where the production seam is
   already explicit enough to avoid creating a second implementation.
3. Promote each replaced tmux edge case into a named deterministic trace with the
   original bug label and seed.
4. Track runtime budgets before and after each migration in `make check` output.

Acceptance criteria:

- no real tmux test is removed without a matching deterministic trace or a
  smaller retained tmux smoke
- `cargo test` keeps the fast fixed corpus cheap enough for normal development
- `make check` remains the place for the wider deterministic corpus and the
  minimal real tmux integration set

Implementation note:

- Added `sync_sim_tmuxbudget_seed_3001...`, `sync_sim_tmuxbudget_seed_3002...`,
  `sync_sim_tmuxbudget_seed_3003...`, and `sync_sim_tmuxbudget_seed_3004...`
  as deterministic protected-layout traces.
- Kept safe-passive real tmux smokes for pane/window movement and focus, and
  ignored the duplicate manual protected-layout tmux variants behind those
  traces.
- Extended the simulator corpus counters and deterministic simulation spec so
  runtime-budget output includes sync preserve/replacement/focus coverage.

## Backlog Order

1. `#autosync` - correctness first; editor navigation must apply without manual
   intervention.
2. `#synclag` - latency second; once correctness is proven, make the slow phase
   measurable and bounded.
3. `#tmuxbudget` - suite-shaping third; migrate only after the behavior and
   latency contracts are stable enough to model.
