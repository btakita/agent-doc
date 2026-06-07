# Plan: fast sync hotpath — responsive pane handoff on editor file switch

## Request (agent-doc-bugs2 head 3, 2026-06-05)

> Significant blocking with the main sync pipeline. Keep the main sync pipeline
> fast so syncing is responsive when I switch files in the editor. Want a hotpath
> to identify if a tmux pane claims a document and show it quickly, or create a
> new tmux pane if there is no claiming pane. The other stuff can be deferred. We
> just need sync to always be responsive and to show the tmux pane. If the tmux
> pane needs time to load and has slower sync tasks, that should not block the
> main sync pipeline.

## Root cause (where the blocking is)

`agent-doc sync` is the editor's file-switch handler. The fast pane-focus
(`safe_passive_focus_actor_after_sync_lock`, `sync.rs:467`) — which resolves the
claiming pane from the local actor projection / controller record and
`select_pane`s it — runs **after** acquiring the sync lock. The lock and the
reconcile phases gate the editor's return:

- `SYNC_LOCK_WAIT_BUDGET = 3s` (`sync.rs:208`) — a file switch can wait up to 3s
  for the lock when another sync/reconcile holds it.
- Stacked phase budgets on the return path: `SYNC_ROUTER_BUDGET` 1s,
  `SYNC_OWNERSHIP_PROOF_BUDGET` 750ms, `SYNC_PRUNE_BUDGET` 1s,
  `SYNC_CONTROLLER_ACTOR_LOOKUP_BUDGET`/`SYNC_PROJECTION_REFRESH_BUDGET` 250ms,
  `SYNC_SAFE_PASSIVE_TOTAL_BUDGET` 1s.

So even though the claiming pane is usually already known (cheap cached read),
showing it is serialized behind lock contention + heavy reconcile work. That is
the "significant blocking" on file switch.

## Why it's not a one-line move

The sync lock exists to prevent concurrent syncs/reconciles from racing while
they restructure panes (stash/swap/reparent). Naively moving the `select_pane`
*before* the lock would let the fast path select a pane that a concurrent
reconcile is about to stash/move — reintroducing the visible-layout races the
passive-sync invariants (CLAUDE.md: "passive mixed-root sync must preserve
visible layout", "fast handoff path after the sync lock") were built to prevent.

## Proposed design (fast hotpath + deferred reconcile)

1. **Pre-lock fast pane resolution + select.** On the safe-passive editor path
   (`run_layout_only` / `--no-autostart` with `--focus`), resolve the focus
   document's claiming pane from the **lock-free cached local projection**
   (`focus::local_actor_projection_pane_for_document`) and `select_pane` it
   immediately, bounded to a tight budget (target ≤150–300ms). If a claiming
   pane exists and is alive, the editor sees it instantly.
2. **Create-on-miss, also fast.** If no claiming pane exists, kick off pane
   provisioning (`route::auto_start`) as a fire-and-forget / bounded step and
   return; do not block the editor on full pane load.
3. **Defer the heavy reconcile.** Move prune, `tmux_router::sync`, ownership
   proof, layout repair, and registry registration behind the lock on a
   **deferred/background** path that does not block the editor's sync return.
   The reconciler remains authoritative for final placement; the fast select is
   an optimistic hint it converges to.
4. **Race safety.** Make the fast pre-lock select **idempotent with the
   reconciler's final placement**: the deferred reconcile re-selects the correct
   pane if it moved one, and the projection cache is updated atomically so the
   fast path never selects a pane mid-stash. Consider a generation/epoch check so
   a stale fast select is corrected by the reconcile rather than fighting it.

## Acceptance

- Editor file-switch sync returns within a tight budget (target ≤300ms) and the
  claiming pane is selected, **even under sync-lock contention or slow pane
  load** — the 3s lock wait no longer gates the visible pane handoff.
- When no pane claims the document, provisioning starts without blocking the
  return; the pane appears when ready.
- All existing passive-sync invariants hold: no destructive layout changes on
  blocked/foreign files, owner guards intact, no pane selected mid-stash, no
  attach-first pane growth.
- Regression coverage in SimWorld for: fast select under contention, create-on-
  miss, and convergence when the reconciler relocates the pane after a fast select.

## Notes / files

- `sync.rs` (`run` 1800, `run_layout_only` 1873, `safe_passive_focus_actor_after_sync_lock` 467,
  `acquire_sync_lock` 257, budget consts 199–208), `focus.rs`
  (`local_actor_projection_pane_for_document`), `route.rs` (`auto_start`).
- Keep `sync.rs`, `SPEC.md`, `README.md`, and the session/tmux command spec
  aligned (sync invariants are spec-bound).
- Sensitive 12K-line hot path with many invariants — implement incrementally
  with SimWorld coverage per step; do not refactor blindly.
