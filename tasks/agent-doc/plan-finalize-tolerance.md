# Plan: finalize should tolerate concurrent unrelated user edits (#fintol)

Operator request (agent-doc-bugs2.md):
> "Finalize should allow the user to type into the document, make unrelated
> changes, compaction, without being rejected. As long as the CRDT plan is not
> affected, the finalize step should be tolerant."

**Run in a CLEAN session (cwd `src/agent-doc`), NOT via `/agent-doc` on this doc** —
it changes the closeout/merge path this doc's controller races. Per the operator's
standing directive, **verify with SimWorld**, not a live editor.

---

## Symptom (recurring all session)

Every finalize in a live JB session logs:
```
IPC proof insufficient ... invariant=live_prompt_drift_after_preflight
recovery=content_ours_snapshot_next_cycle
```
and historically wedged into exit-75 / "visible editor buffer differs from the
expected disk state". The operator types into the document (a new prompt, an
unrelated edit, a Compact Exchange) while the agent is finalizing, and finalize
treats that concurrent edit as drift to fail closed on instead of merging it.

## Current behavior (verified in code 2026-06-11)

The closeout path already has a **two-tier** tolerance, but it is recover-or-reject,
not forward-merge:

1. **Benign absorb (works today).** `ipc_snapshot_would_absorb_live_prompt_drift_after_preflight`
   (`write.rs:513`) returns true — drift is benign — when only non-`exchange`
   components drifted, OR every prompt-bearing candidate change is owned by
   `content_ours`. Then `try_auto_recover_live_prompt_drift` / `live_prompt_drift_auto_recovery_safe`
   (`write.rs:7049/7119`, `#exch-intermix`) write the (superset) snapshot to disk
   and the commit proceeds. This is why most finalizes recover silently.
2. **Fail closed (the gap).** The instant the visible file carries a prompt-bearing
   `exchange` `PromptTarget` or a queue `do [#id]` the snapshot/`content_ours` does
   NOT own (`live_prompt_drift_auto_recovery_safe` returns false; the gate returns
   false), finalize refuses — to avoid silently dropping the user's new content.
   `recovery=content_ours_snapshot_next_cycle` defers it, and the operator hits the
   churn the session memory documents.

So the safety property today is "never drop a concurrent user prompt by adopting
`content_ours`." The operator wants that same content preserved, but via a
**forward merge that commits the response AND keeps the user edit**, instead of a
reject.

## The gap, precisely

The fail-closed case conflates two situations:
- **(a) Collision:** the concurrent user edit touches the SAME region the agent's
  response patch targets (the "CRDT plan"). Merging is genuinely ambiguous → must
  still defer / fail closed.
- **(b) Independent:** the user edit is unrelated to the response patch target
  (a new prompt below, an edit in a different component, a compaction of an
  already-answered exchange tail). The CRDT merge is conflict-free by construction
  — the response and the edit commute. Today this is ALSO rejected, but the
  operator wants it committed: agent response lands, user edit preserved.

## Fix direction (forward CRDT merge, conflict-scoped)

Replace the binary absorb-or-reject at the `live_prompt_drift_after_preflight`
gate with a three-way decision:

1. Compute the response patch's **target span** (the `### Re:` exchange region /
   the components the CRDT plan writes).
2. Compute the concurrent user edit (`candidate` vs `baseline`, minus what
   `content_ours` owns).
3. **If the user edit is disjoint from the response target span** ("CRDT plan not
   affected"): run the existing CRDT merge (`merge::merge_contents_crdt`,
   base = current-at-preflight) of response ⊕ user-edit, commit the union, and
   carry any new user *prompt* forward as the next cycle's uncommitted prompt
   (do not strike/answer it). No exit-75.
4. **If it collides** with the response target span: keep today's fail-closed
   `content_ours_snapshot_next_cycle` behavior (genuine ambiguity).

This generalizes `#exch-intermix` from "snapshot is a superset" to "snapshot and
disk edit are independent → merge", and is the correct closeout-side complement to
the `#rtwatch` read-side fix (base already = newest-of(disk, buffer)).

Compaction (`Compact Exchange`) is the same class: a compaction that only rewrites
an already-answered exchange tail is disjoint from a new response target and should
merge, not conflict (relates to `#w42v` JB-compact-CRDT).

## Rungs (independently shippable; SimWorld-verified per operator directive)

- **#fintol1 — conflict-scope primitive.** Pure fn:
  `response_target_disjoint_from_user_edit(baseline, content_ours, candidate) -> bool`
  in `write.rs`/`merge.rs`. Deterministic unit tests: new-prompt-below = disjoint;
  edit-inside-`### Re:`-target = collision; non-exchange edit = disjoint.
- **#fintol2 — forward-merge at the gate.** Wire the primitive into the
  `live_prompt_drift_after_preflight` decision: disjoint → CRDT-merge + commit
  union + carry the new prompt to next cycle; collision → existing fail-closed.
- **#fintol3 — SimWorld coverage.** Extend `SimEditor`: during a finalize cycle,
  type an unrelated new prompt / compact the answered tail; assert (a) the agent
  response commits, (b) the user's new prompt survives uncommitted for the next
  cycle, (c) NO `live_prompt_drift_after_preflight` reject, (d)
  `strict_closeout_invariants`. Add a collision case that still fails closed.

## Verification

SimWorld only (no live editor). The new `#fintol3` tests are the acceptance bar:
finalize tolerant of independent concurrent edits, still fail-closed on a genuine
collision with the response's CRDT target.

## Files (anticipated)

- `agent-doc-orchestration/src/write.rs` — the `live_prompt_drift_after_preflight`
  gate (`ipc_snapshot_would_absorb_...`, `try_auto_recover_live_prompt_drift`).
- `agent-doc-orchestration/src/merge.rs` — CRDT merge of response ⊕ user edit.
- `src/sim_world.rs` — `#fintol3` coverage.
- `specs/` closeout/merge spec + `SKILL.md`/`runbooks/commit.md` tolerance wording.
