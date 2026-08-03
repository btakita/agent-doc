# Plan: per-component convergence

## Why

Every convergence gate in agent-doc compares the **whole document**: editor
authority against the disk projection, byte for byte. That comparison cannot
succeed while an operator is typing, because the editor buffer is continuously
ahead of disk by construction. The consequence is not a slow drain — it is a
drain that cannot run at all during live editing, which is the mode the product
is built around.

This is the shared root of a family of defects observed on 2026-08-02/03. Each
was patched at its own edge; none of the patches addresses the cause.

| Symptom | Shipped patch | What the patch does not fix |
|---|---|---|
| Authority/disk divergence with no owner stalled the queue forever and stranded two batches of edits on `lazily.md` | 0.35.123 | Only the *unowned* case. A divergence caused by live typing still blocks. |
| Editor paste folded sixteen `do [#id]` lines into one head; fifteen became inert `Freeform` | 0.35.120 | Re-segments after the fact. The paste was never observed as per-item ops. |
| A free-text head was struck because the response echoed it, though the work never ran | 0.35.121 | Catches only responses that *admit* deferral. |

The second row is the tell. `op_log` recorded **zero** per-item operations for a
sixteen-item operator paste. There is no per-component operation stream, so
reconciliation has only whole-document text to work from, and every guard built
on top inherits that granularity.

Tracked previously as the per-cell CRDT merge intent (`qcellmerge` in the
operator memory index); this plan is that work, scoped and sequenced.

## The write path still asserts the claim session-check no longer does

`crdt_relay_pending_refusal` (`agent-doc-git-io/src/live_buffer_guard.rs`) is the
refusal printed by `write --commit` when the commit defers on relay convergence.
It states unconditionally:

> The capture is already durable and the same intent commits itself once
> delivery converges — this is a deferral, not a lost response. […] do NOT
> re-send the response, force disk, `admin recycle`, or `admin reload-lib`

Its own doc comment records why the wording is emphatic: on 2026-07-26 a bare
`Error:` with no next step made two sessions invent recoveries that each
perturbed the capture being awaited. That reasoning is sound, and the guard
should stay — but the promise is only true while a durable owner exists.

Observed twice on 2026-08-03, adding backlog items to `agent-doc-bugs2.md`: the
mutation applied, `write --commit` printed this refusal, and `session-check` —
carrying the 0.35.123 fix — contradicted it:

> NO cycle is open and NO response capture is retained, so nothing owns this
> divergence and no state edge will fire — the visible edits are STRANDED, not
> deferred, and waiting will not commit them.

`session-check` was right both times; each was recovered with `agent-doc commit`.
So the two paths now disagree about the same state, and the one an agent hits
*first* is the one that is wrong. A session that obeys it waits forever on a
state edge that cannot fire.

Fold into this work rather than patching separately, because the correct
predicate is the per-component one: whether anything owns **the components this
turn wrote**. Until that exists, apply the same ownership test 0.35.123 added to
`session-check` — no open cycle and no retained capture means stranded, not
deferred — so the two paths stop contradicting each other. The "do NOT re-send /
recycle / force disk" guidance stays intact for the genuinely-owned case; only
the unowned case gains a remedy.

## The invariant to establish

> A turn requires only the components it **owns** to be settled. An operator
> edit to a component the turn does not own must never block closeout, and must
> never be overwritten by it.

Concretely: an agent writing `exchange` and `backlog` must be able to commit
while the operator is mid-edit in `queue`, and the operator's queue edit must
survive that commit untouched.

## Phases

Each phase must be independently shippable and independently red-green. Do not
collapse them: the failure mode of this area is a large change that cannot be
bisected when it corrupts a live document.

### Phase 1 — observe per-component ops

Make the editor→controller path record operations per component, so
`op_log` has a row per changed item rather than one whole-document text swap.
Success criterion is empirical, not structural: replay the sixteen-line paste
from 2026-08-03 and assert `op_log` contains sixteen queue-item inserts. Today
it contains zero. Until that assertion passes, no later phase has a substrate.

### Phase 2 — per-component authority hashes

Extend the authority/disk comparison from one document hash to a hash per
component. Keep the whole-document hash as a derived value so existing
diagnostics keep working. No behavior change in this phase — the point is to be
able to *say* which components diverge before anything acts on it.

### Phase 3 — scope the convergence gate to owned components

Change the gates that currently require whole-document equality
(`decide_read_only_terminal_projection` and the `session-check` divergence
INTERRUPT are the two that stalled documents this week) to require equality only
on the components the turn wrote. A divergence confined to an unowned component
becomes a normal, non-blocking condition.

This is the phase that makes the queue drain during live editing. It is also the
phase that can destroy an operator buffer if the ownership set is computed too
broadly, so it needs the adversarial tests below before it ships.

### Phase 4 — per-component commit

Commit only the owned components, rebasing onto whatever the operator has since
written elsewhere in the document, instead of writing the whole file.

## Tests that must exist before Phase 3 ships

The guard against this whole class is proving that an operator edit **survives**,
not that a commit succeeds. Assert survival explicitly:

- SimWorld: agent writes `exchange` while a simulated operator edits `queue` on
  every tick. Assert the commit lands **and** every operator keystroke is present
  afterwards. Run it for enough ticks that an interleaving is actually exercised
  rather than assumed.
- Ownership over-claim: force the turn's owned-component set to include `queue`
  when it should not, and assert the operator edit is dropped — i.e. prove the
  test can fail. A survival test that has never gone red is not evidence.
- Replay the three real 2026-08-03 documents (`lazily.md`, `haiven-dev.md`,
  `agent-doc-bugs2.md`) at the hashes recorded in `state.db` `state_events`, and
  assert none of them reaches a permanent INTERRUPT.

## Explicit non-goals

- Do not relax the rule that the agent never component-merges a disk candidate
  under a live editor. Per-component scope narrows *what* must converge; it does
  not grant new authority to overwrite.
- Do not remove the whole-document hash. Several diagnostics and the boundary
  invariant still key on it.

## Risk

The realistic failure is a wrong ownership set silently discarding operator
text, which is the one outcome every guard in this subsystem exists to prevent
and the one hardest to notice after the fact. Phase 3 should ship behind a
config flag, default off, until the survival tests have run against real
multi-session use for a period the operator judges sufficient.
