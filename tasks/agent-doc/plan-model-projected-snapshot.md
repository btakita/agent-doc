# Plan: model-projected snapshot baseline (#mps) — successor to #snbc

**One-line goal:** make the structured document model (markdown-AST / overlay CRDT)
the *authoritative source of the merge baseline* for every doc format, projecting
the snapshot/baseline on demand from a per-cycle pinned model version — instead of
authoring a separate `.md` baseline file that can drift.

**Run in a CLEAN session (cwd `src/agent-doc`), NOT via `/agent-doc` on a session doc** —
it touches the closeout/merge path.

---

## Relationship to #snbc (it is the *replacement*, not the next step)

#snbc and #mps are alternatives on the same axis — *where the merge base lives*:

- **#snbc:** base = **git HEAD**. Wrong: the base must carry the uncommitted new user
  prompt (preflight commits the prior cycle but leaves the new prompt uncommitted), so
  `base ≠ HEAD`. (See `plan-snapshot-baseline-consolidation.md` audit.)
- **#mps:** base = **the document model**, projected on demand, pinned to a per-cycle
  version. Right: the pinned overlay version = HEAD + the uncommitted prompt.

**#mps does not depend on any #snbc code.** The only reusable #snbc output is the
Slice-1 read-site audit (already landed), which classifies every snapshot/baseline
read as either *merge-base (projectable)* or *in-flight synthetic (stays)*. #mps starts
from that table.

**#mps is already half-shipped for CRDT docs.** `snapshot::crdt_merge_base_state`
(`snapshot.rs:761`) already builds the merge base by projecting the overlay model
(`OverlayCrdtDoc::to_markdown()`) when its projection matches the cycle baseline, with a
text fallback when stale. Callers: `apply_template_response` (`run.rs:1611`), `run_stream`
/ `run_ipc` (`write.rs:9221/9518/10003`). #mps generalizes this proven path to all formats.

## What the model can and cannot project (from the #snbc audit)

| Snapshot/baseline role | Projectable from the model? |
|------------------------|------------------------------|
| merge base / next-cycle baseline (current-at-preflight) | **Yes** — pin overlay version at preflight, project it |
| diff-against-baseline (user-edit display) | **Yes** — same pinned projection |
| `content_ours` (baseline + response − user edits) | **No** — synthetic merge input, not any doc version |
| dedup / over-application repair intermediates | **No** — synthetic remediation states |
| capture-replay hash validation | Yes *iff* projection is byte-stable (hard gate, rung 1) |
| init / rename migration | unchanged (keep snapshot store) |

The synthetic intermediates keep their own store regardless. #mps only re-homes the
*baseline/merge-base* role to the model.

## Hard preconditions (the three obstacles, made into gates)

1. **Frozen point-in-time.** Never trust a live mutable model as the frozen base. Always
   pin a CRDT version / state-vector at preflight and project *that*. (Mirrors
   `crdt_merge_base_state`'s discipline: trust the overlay only when its projection equals
   the explicit cycle baseline.)
2. **Byte-stable projection.** `capture.rs` replay re-validates the baseline *hash*.
   `model → markdown` must be byte-identical across versions / tombstone GC / reordering.
   This is fragile (see the existing `overlay_merge_base_is_order_stable_for_exchange_append`
   regression test). Rung 1 proves it on real traffic *before* any cutover.
3. **Universality.** The overlay model is maintained today only for `agent_doc_write: crdt`
   docs (`apply_template_response`, `stream_loop`, compact, closeout rebuild). Inline and
   plain-template docs have no overlay — the `.md` snapshot is their only baseline. They must
   reach overlay-maintenance parity before their base can flip.

## Rungs

**Rung 0 — audit (DONE).** #snbc Slice-1 read-site classification. Feeds the table above.

**Rung 1 — maintain the overlay model on every write path, in shadow (no behavior change).**
- Persist `OverlayCrdtDoc` alongside the existing `.md` snapshot on the inline and
  plain-template write paths too (not just CRDT) via `save_document_crdt`. Do **not** read
  from it yet.
- Add a projection-equivalence invariant on each write: assert/log that
  `OverlayCrdtDoc::from_markdown(snapshot).to_markdown() == snapshot` and that the persisted
  overlay projects byte-identically to the `.md` snapshot. Emit a grep-able
  `mps_projection_equiv ok|drift len=… file=…` ops.log marker.
- Deterministic evals for inline / template / exchange-append / queue / boundary shapes —
  the byte-stability gate (obstacle #2) proven offline + on live traffic before cutover.
- Exit gate: zero `mps_projection_equiv drift` markers over a real dog-food run.

**Rung 2 — pin a per-cycle model version at preflight; project the base; cross-check.**
- Where `save_baseline_content` runs (`preflight.rs:3196`), also encode + persist the
  overlay state-vector / version handle for the cycle baseline.
- Compute the merge base from the pinned version's projection, but keep the `.md`
  snapshot/baseline as the *authority*; compare and log divergence (generalized
  `crdt_merge_base_state` "overlay matches fallback" guard).
- Exit gate: projected base == `.md` base across the dog-food corpus; replay-hash validation
  unaffected.

**Rung 3 — flip merge-base + diff reads to the projection (baseline file becomes derived).**
- `read_explicit_baseline` (`write.rs:1091`), `template_patch_application_base`,
  `compute_diff_with_current` source the base from the pinned model version's projection.
- The `.md` **baseline file** (`.agent-doc/baselines/`) becomes derived → removable. (This is
  the correct, narrowed version of #snbc Slice 4 — baseline file only, not the snapshot.)
- `content_ours` + repair intermediates stay synthetic — unchanged.
- SimWorld coverage for the prompt-bearing finalize-merge class is a precondition.

**Rung 4 — snapshot `.md` becomes a derived cache (or drop) for the baseline role.**
- For merge-base purposes the `.md` snapshot is now a projection; keep it only as a
  human-readable debug cache or drop it. In-flight synthetic stores (content_ours, repair,
  capture) retain their own persistence.
- Net result the operator asked for: the document model is the source of truth for the
  baseline; no independently-authored `.md` baseline that can drift.

## Invariants

- base = current-at-preflight (HEAD + uncommitted prompt) = the **pinned overlay version**,
  never bare git HEAD.
- byte-stable projection is a hard precondition: rung 1 green on real traffic gates rung 3.
- non-crdt docs reach overlay-maintenance parity (rung 1) before their base flips (rung 3).
- synthetic merge intermediates (`content_ours`, repair) are never claimed to be model
  projections.

## Related

Extends `#rtwatch` / `#rtwfeed` / `#rtwwire` — the model is already the *read* authority;
#mps makes it the persisted *baseline* authority too. Supersedes `#snbc` (wrong axis: base
in git HEAD). Builds directly on `snapshot::crdt_merge_base_state` +
`agent_doc_markdown_ast::crdt::OverlayCrdtDoc`.
