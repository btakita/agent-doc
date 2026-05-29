# Plan: queue / IPC-buffer / snapshot drift loop (`stuck_captured_cycle` + `inactive_queue_residue`)

## Symptom (reproduced on `tasks/agent-doc/open-code.md`, 2026-05-29)

After a queue item is consumed and the auto-queue halts (`queue_halted: item_modified`),
every subsequent `/agent-doc <FILE>` invocation surfaces, with **no user edit**:

- a perpetual 1-line working-tree-vs-HEAD diff in the `agent:queue` component,
- a recurring `stuck_captured_cycle` warning (a captured/committed response body
  reported "not present in HEAD"),
- an `inactive_queue_residue` warning,
- and the editor-buffer guard (`visible editor buffer ... differs from expected disk state`)
  on `write --commit`.

`agent-doc commit` "reconciles" it but the diff regenerates on the next preflight,
so the cycle never converges. Committing the working state with raw `git commit`
also regenerates. `git checkout <FILE>` (discarding the working buffer) **does**
break the loop, and from a clean HEAD `preflight` is stable (`no_changes: true`).

## Root cause (diagnosis)

Three subsystems interact badly when a doc is owned by a live
`agent-doc start --route-owned` process (here PID 138671) + controller:

1. **Queue halt strands live items.** When the auto-strike maintenance halts the
   queue (`item_modified`) after consuming a done item, it strips `auto` / clears
   `queue_active` but leaves remaining **live** prompts (`#adoc-orch-shim-cleanup`)
   in the now-inactive queue → `inactive_queue_residue`. `need_clear_proven_non_auto_residue`
   in `preflight.rs` only clears *fully drained* residue, so a half-drained halted
   queue is left in a non-executable-but-non-empty state.

2. **IPC working buffer diverges from snapshot/HEAD and is not reconciled.** The
   route-owned process keeps an IPC working buffer. Queue-maintenance mutations
   (auto re-add / item state) land in that buffer, while `agent-doc commit`
   commits the *snapshot*. The two diverge by ~1 line and nothing reconciles the
   working buffer back to the committed queue shape (commit logs show
   `skipping working-tree boundary reposition — IPC listener active` +
   `IPC reposition boundary signal sent`). Each preflight reads the divergent
   working buffer and re-emits the diff.

3. **Capture-ledger `stuck_captured_cycle` keys off the same divergence.** Because
   the snapshot the capture-replay hash validated against no longer matches the
   working/HEAD shape (queue churn), the post-commit guard reports the prior
   response "not present in HEAD" even though it is committed.

## Proposed fix (direction — needs a repro harness first)

1. **Build a deterministic repro** in a SimWorld/integration test: doc owned by a
   simulated IPC listener, auto-queue with `[done-item, live-item]`, run a
   consume→halt cycle, then assert a follow-up preflight is `no_changes` (no queue
   re-diff, no `stuck_captured_cycle`).

2. **Reconcile the IPC working buffer to the committed queue shape** after queue
   maintenance / commit: when the commit path skips the working-tree reposition
   because an IPC listener is active, it must still send the listener the
   *post-commit queue body* (not just the boundary signal) so the buffer converges.
   Alternatively, preflight queue-maintenance mutations should be folded into the
   same snapshot/commit boundary as pending maintenance instead of only the
   working file.

3. **Halt should not strand live residue.** On `item_modified` halt, either keep
   the queue active on the *new* head, or move the un-run live items back to a
   clearly inactive shape that preflight treats as `no_changes` (don't warn +
   don't re-diff). Decide intended semantics: does a halted auto-queue retain its
   live tail, or is the tail returned to backlog-only?

4. **Decouple `stuck_captured_cycle` from queue churn.** The capture-replay guard
   should validate the response body against the exchange/HEAD response content,
   not a whole-snapshot hash that queue-component churn invalidates.

## Progress (2026-05-29)

- **`enqueue_route_dispatch_prompt` exonerated.** A repro test mirroring the exact
  live queue shape (`preset` + 2 `Completed` residue + 1 live prompt under `auto`,
  re-dispatching the live head) confirms it does **not** duplicate
  (`route_enqueue_dispatch_prompt_no_dup_with_completed_residue_and_live_head`).
  So the duplicate `do [#adoc-orch-shim-cleanup]` is **not** an enqueue bug — it is
  a **CRDT / 3-way merge artifact** of the divergent IPC-buffer vs snapshot queue
  body (consistent with root cause #2).
- **Mitigation landed (converges + caps the symptom):** `queue::dedup_live_prompts`
  collapses duplicate live `Prompt` entries (two identical live prompts is never
  valid), wired into `preflight::run_queue_maintenance` *before* activation/strike.
  Preflight now collapses any merge-duplicated head and is **idempotent** after
  dedup, so the drift no longer grows on each cycle. Tests:
  `dedup_live_prompts_*` (queue), `preflight_collapses_duplicate_live_queue_prompt`.

### Root cause #1 addressed (2026-05-29) — residue warning is now per-edit, not per-preflight

The recurring `inactive_queue_residue` warning was the *every-preflight-with-no-
user-edit* symptom. Root analysis: the warning fired purely from the static
"inactive + non-empty queue" state, so a stable post-halt queue (the live tail an
`item_modified` halt retains and syncs into the snapshot) re-warned on every
preflight forever. An earlier attempt to suppress the warning whenever live
prompts remained was **wrong** — it broke the legitimate Scenario B
(`test_preflight_warns_but_does_not_target_inactive_queue_edit`: a user adds a
live `do [#id]` to an inactive queue and must be warned it will not run), and the
two states are indistinguishable in steady state.

The correct, narrow fix: `inactive_queue_residue` is now scoped to *genuine
edits* — preflight emits it only when the inactive queue body changed vs the
snapshot this cycle (`inactive_queue_changed_vs_snapshot`, normalized through
`queue::parse`+`queue::render`). A stable inactive queue unchanged from the
committed snapshot is not re-flagged. This removes the recurring warning that fed
the drift loop while preserving the warning on real inactive-queue edits. Tests:
`preflight_does_not_reflag_stable_inactive_queue_as_residue`,
`preflight_flags_inactive_queue_when_changed_this_cycle`,
`test_preflight_warns_but_does_not_target_inactive_queue_edit` (unchanged, still
green). Spec: `specs/07-orchestration-commands.md` § Post-commit queue consumption.

### Incidental test-isolation fix (2026-05-29)

`codex_bare_run_inside_owning_pane_fails_before_nested_dispatch`
(`tests/run_integration.rs`) failed whenever the suite ran from inside a Claude
Code session: `detect_harness()` checks Claude/OpenCode markers before Codex, so
an inherited `CLAUDECODE` short-circuited the codex recursive-deadlock guard. The
test now clears the competing harness markers so it deterministically simulates a
Codex harness from any environment. Not part of the drift loop, but required for a
green `make check` from the agent-doc session harness.

### Still open (root cause — needs the repro harness)

- The **merge divergence itself** (why the IPC buffer re-activates `auto` + a
  duplicate vs the committed inactive snapshot) is not yet fixed — dedup caps the
  damage but doesn't stop the divergence. Still needs items #2/#4 above. The
  durable fix is to push the halt's queue-shape change (strip `auto`, clear
  `queue_active`) **through the IPC listener** so the IDE buffer converges,
  rather than only writing to disk where the plugin's stale in-memory buffer
  wins on its next flush.
- A **running pre-fix `start --route-owned` process** keeps the old binary; it must
  be restarted for any fix to take effect on a live session.

## Acceptance

- Repro test red before, green after.
- After a consume→halt cycle on an IPC-owned doc, repeated `preflight` is
  `no_changes` with no `stuck_captured_cycle` / `inactive_queue_residue`.
- No raw `git checkout` needed to break the loop.

## Notes

- Immediate symptom on `open-code.md` was resolved 2026-05-29 by `git checkout`
  (discarding the stale IPC buffer); tree is stable. This plan is the durable fix.
- Out of scope but adjacent: `make check` does not run the `agent-doc-orchestration`
  isolated suite (`#adoc-make-check-orch-tests`).
