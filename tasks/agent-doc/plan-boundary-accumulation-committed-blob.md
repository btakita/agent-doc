# Plan: #boundaryaccum1 — enforce single-boundary invariant on the committed blob

## Problem (observed 2026-07-01 in agent-doc-bugs2.md)

The exchange carries **three** `<!-- agent:boundary:… -->` markers scattered
mid-component (HEAD + snapshot both):

- `d0dd7c4e` after the "Re: Pre-run queue edits" response
- `qactsync1` after the "Re: Realtime sync" response
- `ftimm0001` after the "Re: Free-text prompts" response

The invariant (asserted by the `ffi.rs` boundary tests, `count == 1`, and the
aspirational `reposition_boundary_removes_stale` / `apply_patches_boundary_invariant`
notes in `agent-doc-template/src/template.rs`) is **exactly one** boundary, at the
end of `agent:exchange`. Three accumulated in committed content.

## Recurrence (2026-07-01, laura-s.md — working-tree-only variant)

Second observation, different trigger. `laura-s.md` accumulated **5** boundaries
(`a7915672` orphaned mid-exchange after a binary-authored `ipc_proof_insufficient`
diagnostic block, plus `513621c4`/`d8765079`/`7e731413`/`c83c9f64` stacked at the
end) **only in the uncommitted working tree** — `HEAD` stayed clean at exactly one
boundary (`f13ef957`). The corruption also duplicated the `(HEAD)` live-editor
marker across 4 `### Re:` headings and orphaned status-context bullets into the top
of the exchange.

Key distinction from the original observation: here the **commit-time collapse
worked** (HEAD = 1 boundary), but the **`ipc_visible_repair` disk-writeback during
interrupted-cycle recovery did not collapse** — it re-emitted the exchange with an
appended boundary per repair attempt. So the self-healing invariant must also cover
the *recovery/repair writeback path*, not just the pre-commit staging path. No live
supervisor was running; recovery was a clean `git checkout HEAD -- <doc>` (HEAD was
authoritative and lost no operator content).

## Root cause

`reposition_boundary_in_snapshot` (`agent-doc-orchestration/src/git.rs:1613`) is
the canonical post-/pre-commit collapse (strip all boundaries, insert one fresh at
end). It has two guards that *hand ownership away*:

1. **Active-run guard (git.rs:1614-1622):** if a `pending_response_path` exists
   (a `finalize`/`write` is in flight), the whole function returns early — "the
   IPC path owns the transition." The pre-staging collapse call (git.rs:1189) is
   the same function, so during a finalize-driven commit the staged blob is *not*
   collapsed by git.rs; collapse is expected to happen inside the finalize
   apply_patches pre-patch (strip-all + insert-one).

2. **IPC-listener guard (git.rs:1656-1662):** even outside an active run, the
   *working-tree* rewrite is skipped when a live IDE listener is present,
   delegating to a post-commit IPC reposition signal the plugin applies.

The failure chain that left 3 boundaries in HEAD: over the last few cycles the
live JB plugin was **stale** (`stale_install`: cdylib predates the source edits)
and finalize **wedged** on editor-IPC churn (`live_prompt_drift_after_preflight`,
duplicate-prompt ack). The operator's own notes in those responses say the
responses were "landed via a direct commit." So:

- The finalize/IPC path that "owns the boundary transition" never delivered the
  collapse (wedged / bypassed), AND
- the IPC reposition signal was delegated to a stale plugin that never applied it, AND
- git.rs's own collapse was skipped by the active-run guard.

Net: each wedged cycle appended a new boundary without stripping the prior one,
and the staged→committed blob accreted one boundary per cycle. **Nothing
re-enforces the single-boundary invariant on the binary's own committed
artifact** when the delegated path fails.

Distinct from `#fullboundary` (which gates *concurrent writers* to reduce the
wedge trigger): even with the convergence gate, the committed-blob invariant is
still delegated and can be silently violated. `#fullboundary` reduces the
frequency; this item makes the invariant self-healing regardless of write path.

## Fix

- [ ] Separate the two concerns in `reposition_boundary_in_snapshot` / the
  staging path: the **staged git blob and snapshot are binary-owned artifacts**
  — collapsing boundaries in them never races the live editor buffer (git stages
  a snapshot, not the live file). Always collapse boundaries to exactly one in
  the snapshot/staged blob before commit, independent of the active-run and
  IPC-listener guards. Keep those guards scoped to the **working-tree disk
  rewrite** only (that one does race the editor and is correctly delegated via
  IPC when a listener is present).
- [ ] Add a **post-commit invariant check**: if the committed blob contains >1
  `agent:boundary` marker, that is a violation — reconcile with a binary-owned
  follow-up collapse commit (or fail closed loudly with a phase-labeled error)
  rather than carrying it forward. This catches the direct-commit / bypassed-IPC
  path that skipped collapse.
- [ ] Promote the aspirational template invariants to real regression tests:
  `apply_patches_boundary_invariant` (post-apply, exactly one boundary at end of
  exchange) and `reposition_boundary_removes_stale` (N stale boundaries → 1).
- [ ] SimWorld: N cycles of wedged-finalize → direct-commit must never leave >1
  `agent:boundary` in HEAD; a stale/wedged editor consumer that never applies the
  IPC reposition signal must not defeat the committed-blob invariant.

## Acceptance

- `make check` green (new template + git regression tests).
- `make tmux-ci` green: a live wedged-plugin drain still converges HEAD to a
  single boundary.
- Repro doc (`agent-doc-bugs2.md`) collapses to exactly one boundary and stays
  there across subsequent cycles.

## Notes

Live-supervisor-critical seam (edits the commit/boundary path). Land the
binary-owned staged-blob collapse + post-commit invariant check + unit/SimWorld
coverage in a focused cycle with `make check` + `make tmux-ci`; the live-editor
eyeball is the only operator-gated step. Compose with `#fullboundary` (Phase 2
convergence gate) rather than replacing it.
