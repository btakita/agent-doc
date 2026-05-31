# Plan: `do [#id]` partial closeout must rewrite the remaining state

## Live repro

- Document: `src/boost-client/tasks/monsterrodholders.md`
- Prompt: `do [#shoplabels]. #spec-test-commit-push`
- Outcome:
  - repo-backed Google for WooCommerce feed-label code was implemented,
  - tests/spec were updated,
  - submodule and parent pointer were committed and pushed,
  - the response explicitly said deploy / live Merchant Center sync were not run because the prompt preset was not `#spec-test-commit-push-deploy`.
- Bad state:
  - `#shoplabels` remained open, which is correct because live deploy/sync/coverage verification remained,
  - but the backlog item and queue head still looked like the original full task instead of the next live phase,
  - the user had to ask whether `#shoplabels` was done.

This is distinct from `#do-id-closeout-open-backlog`: that bug covers a closeout
that claimed a tracked item was complete while the id remained open. Here the id
legitimately stayed open, but the system allowed a stale open state with no
required `--pending-edit`, `--pending-gate`, or explicit next-phase queue update.

## Invariant

When a `do [#id]` turn successfully completes only part of the item, closeout
must leave a machine-visible lifecycle outcome:

1. `--done <id>` if the acceptance criteria are fully satisfied,
2. `--pending-gate <id>` if implementation is done but review/external validation remains,
3. `--pending-edit id=...` if the item stays open with a narrower remaining scope,
4. or an explicit typed "stays open because ..." marker that preserves/requeues
   the id with the next required action.

Leaving the exact original backlog text and queue head after a partial success
should fail closed or trigger a clear repair instruction.

## Proposed implementation

1. Extend prompt-target tracking for `do [#id]` to distinguish complete, gated,
   and partial outcomes.
2. Add a session-check guard:
   - if the response says work was committed/pushed but also says deployment,
     external sync, live verification, or user approval remains,
   - and the target id remains `[ ]`,
   - require either `--pending-edit`, `--pending-gate`, or a typed open-state
     marker that mutates the visible item/queue.
3. Make queue convergence preserve a revised next-phase prompt when a partial
   closeout narrows the remaining work.
4. Add deterministic tests:
   - partial repo-only closeout with unchanged backlog/queue -> fails,
   - same closeout with `--pending-edit` narrowing the item -> succeeds,
   - same closeout with `--pending-gate` -> succeeds,
   - full closeout with `--done` -> succeeds,
   - response merely citing "not deployed" for an unrelated id -> no false positive.
5. Update runbooks so agents know that a `#spec-test-commit-push` preset can
   complete a repo phase without completing a live/deploy backlog item.

## Landed (WARN-only slice, 2026-05-30)

- `check_partial_closeout_state_guard` (`session_check.rs`, wired into the
  committed-cycle guard loop): WARN-only. Fires when a `do [#id]` turn's response
  carries a shipped signal (committed + pushed) **and** a tight "remaining live
  work" phrase (`PARTIAL_CLOSEOUT_REMAINING_PHRASES`: not deployed, live
  verification, external validation remains, awaiting approval, feed sync,
  merchant center, …) **and** a directed id is still `[ ]` open in
  `agent:backlog`. It advises narrowing the item + queue head to the next phase
  via `--pending-edit` / `--pending-gate`. Suppressed by a
  `<!-- no-partial-closeout-guard -->` response marker.
- Deterministic tests: warns on shipped+remaining with an open directed id;
  silent without a remaining signal; suppressed by the marker; silent when the
  target was reaped (`--done`). Spec: `specs/07-closeout-commands.md`.
- Chosen WARN-only (never strict) because this guard sits on the closeout path
  the auto-queue drain itself uses; a false-positive strict block would wedge the
  queue.

## Remaining (deferred)

1. Per-id complete/gated/partial outcome tracking in `cycle_state` (distinguish a
   narrowing `--pending-edit` from a bare keep-open / gate) so the guard fires
   only when the text was genuinely *not* narrowed, removing the need for the
   suppression marker.
2. Opt-in strict mode once (1) makes detection precise enough to fail closed.
3. Queue convergence that carries a revised next-phase prompt forward when a
   partial closeout narrows the remaining work.
4. Runbook note (plan step 5): a `#spec-test-commit-push` preset can complete a
   repo phase without completing a live/deploy backlog item — narrow the item,
   don't leave the full-task text.

## Acceptance

- A future `#shoplabels`-shaped turn cannot leave the user wondering whether the
  item is done.
- The visible backlog and queue always describe the next action after a partial
  `do [#id]` closeout.
