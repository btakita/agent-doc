# Plan: CRDT overlay must preserve live keystrokes (no silent data loss)

Status: filed 2026-06-26 from a live dogfood production bug (alex-lee.md). Owner: focused cycle.

## Symptom (operator, alex-lee.md, 2026-06-26)

1. Typed `model: opus` / `claude_model: opus` into the frontmatter of a freshly-created
   `alex-lee.md`; the CRDT **deleted** what was typed.
2. Intermittent **1-character deletes while typing**.

These are **data loss**, not lag — distinct from the already-shipped `#qbasehashmemo`
fix (0.34.49), which only removed the per-keystroke whole-doc base-hash *recompute*
(a CPU/lag fix). `#qbasehashmemo` does not touch which content the merge base keeps.

## Root cause

`crdt_merge_base_state` — `agent-doc-orchestration/src/snapshot.rs:1121-1204`.

When the structured live-buffer **overlay** sidecar decodes but its markdown
projection `!= fallback_markdown` (the explicit cycle baseline), the code classifies
the overlay as **stale**, logs `crdt_merge_base_overlay_stale`, and
`rebuild_overlay_crdt_locked(&path, fallback_markdown)` — i.e. **rebuilds the overlay
from baseline, discarding the overlay's content** (`snapshot.rs:1138-1173`,
`CrdtMergeBaseSource::FallbackOverlayProjectionMismatch`).

The conservative discard exists to avoid "replaying old content as a fresh concurrent
insertion" (the comment at `snapshot.rs:1117-1120`). But `markdown != fallback_markdown`
conflates two opposite cases:

- **overlay-behind (stale):** overlay holds *older* content than baseline → discarding is correct.
- **overlay-ahead (live):** overlay holds *newer* live keystrokes the user just typed,
  not yet in `fallback_markdown` → discarding **drops the user's edit**. ← the bug.

Plain string inequality cannot tell these apart, so live keystrokes typed in the
preflight→finalize window are silently dropped when convergence rebuilds the base.

Log evidence (alex-lee.md, 2026-06-26T18:16:07Z): `crdt_merge_base_overlay_stale` →
`crdt_merge_base_overlay_rebuilt` → `editor_op_recorded kind=delete offset=2342
delete_len=8` + `kind=insert offset=2342 insert_bytes=8` (`#qnodemerge4wire`).

## Fix design

Distinguish overlay-ahead from overlay-behind using CRDT causality, not string equality.

1. **Causal comparison, not text equality.** Compare the overlay CRDT state vector
   against the baseline-derived state. If the overlay strictly *dominates* the baseline
   (every baseline change is causally present in the overlay, plus more), it is
   **ahead** → keep the overlay (it carries live keystrokes) and CRDT-merge the baseline
   into it rather than discarding. Only when the overlay is concurrent-divergent or
   strictly behind do we fall back to baseline.
2. **3-way CRDT merge on the ahead case.** Use the existing CRDT merge (`merge.rs`,
   `crdt::merge`) with the shared committed snapshot as the common ancestor so
   overlay-ahead keystrokes survive and baseline-only changes still land — conflict-free,
   no "replay old content" because the ancestor anchors deletions.
3. **Never silently discard.** When the overlay *is* discarded (genuinely behind /
   undecodable), log the **byte diff** of what was dropped (`overlay_discarded_bytes`,
   first/last differing offsets) so future data loss is never silent — directly satisfies
   the operator's "production bugs should be immediately visible" directive.
4. Keep the per-cell isolation track (`#qcellmerge1`) as the longer-term substrate; this
   fix is the targeted correctness stop-gap on the whole-doc overlay path.

## Edge-case test matrix (dogfood-loop improvement — `#crdtedgetests`)

Deterministic SimWorld / unit coverage (no live tmux/editor needed). Each row is a test:

| # | Scenario | Expected |
|---|----------|----------|
| 1 | Overlay ahead by 1 inserted char during a concurrent finalize | char preserved |
| 2 | Overlay ahead by a multi-line frontmatter block (the alex-lee repro) | block preserved |
| 3 | Overlay strictly behind (older content) | baseline wins, overlay discarded + diff logged |
| 4 | Overlay concurrent-divergent (edit in same region as baseline) | CRDT-merged, both survive, no dup |
| 5 | Empty doc + first keystroke before first cycle observation | keystroke preserved (`#qftloss` adjacency) |
| 6 | Rapid burst: N keystrokes interleaved with a convergence pass | all N survive, no 1-char drop |
| 7 | Overlay decode error | baseline fallback + `overlay_discarded_bytes` logged |
| 8 | Frontmatter edit while exchange convergence runs | frontmatter + exchange both survive |

Add these under the SimWorld engine CRDT coverage (`src/sim_world/`) and a
`snapshot.rs` unit module for `crdt_merge_base_state` directly.

## Supervisor recycle spin-loop (`#suprecyclespin`, coupled)

Separate but same incident: `supervisor_recycle_deferred_cycle_open` fires ~2/sec
indefinitely (`idle_watch.rs:1508`, gated by `supervisor_recycle_action` cycle_open
arm at `decisions.rs:563`). The in-memory `cycle_open` flag stays true for an *old*
turn (`cycle-1782497652506`) after a *newer* cycle (`synthetic-1782497767123`)
committed, so the recycle never reaches its boundary. This is the unbuilt
`#midturnresumeb` (Phase B) — a stalled/abandoned cycle with `inflight=0` past a
deadline must be *resolved* (resume or force-close) instead of deferred forever.
Fix: in the recycle-defer path, treat `cycle_open && inflight==0 && stalled_secs > T`
as resolvable — clear the stale in-memory cycle gate so a newer committed cycle
supersedes an abandoned older turn id.

## Process: immediate production-bug logging

Per operator directive (2026-06-26): production CRDT bugs observed during dogfooding are
logged in `tasks/agent-doc/agent-doc-bugs2.md` **the same turn** they're observed (with
ops.log marker evidence + code location), then fixed in a focused cycle. This plan is the
template for that flow.
