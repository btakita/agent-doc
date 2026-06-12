# Plan: a clean `/agent-doc` exchange run (zero IPC drift) — #cleanrun

**Question answered (agent-doc-bugs2.md, 2026-06-11):** *"Can I run the fixes in an
exchange session or do I need a pure Claude session?"*

**You need a pure Claude session** (cwd `src/agent-doc`, NOT invoked via `/agent-doc`
on this doc). The drift/closeout fixes cannot run inside an `/agent-doc` exchange
session on `agent-doc-bugs2.md` for two reasons proven repeatedly this session:

1. **The controller races every write.** `#fintol`, `#pcwc`, `#rtwbcast` modify the
   exact finalize/commit/IPC path the exchange session uses to persist its own
   response. The live supervisor races each write — that is why every finalize this
   session hit `live_prompt_drift_after_preflight`, one corrupted the worktree
   (`postcommit_worktree_check match=false`), and one dropped a queue strike.
2. **Testing swaps the binary mid-cycle.** Verification needs `make install` +
   `agent-doc lib-install` + recycling controllers, which replaces the running
   binary the live session depends on.

What an exchange session CAN do (and all this session did): capture plans/backlog,
answer questions, work on unrelated repos. Implementation + install + verify must be
a pure Claude session.

---

## Definition of "clean run" (exit criteria)

An `/agent-doc tasks/...md` exchange cycle that finalizes with, in `ops.log`:
- NO `live_prompt_drift_after_preflight`
- NO `postcommit_worktree_check match=false`
- NO lost queue strike (the consumed head is struck in HEAD)
- NO exit-75 / `content_ours_snapshot_next_cycle` recovery
- `session-check ok ... committed` with a clean worktree, no manual `git checkout HEAD`.

---

## Phase 0 — refresh the install and RE-BASELINE the drift (do FIRST) — #cleanrun0

**The single most important step, and possibly most of the fix.** The live
`~/.cargo/bin` binary + JB cdylib **predate every committed drift fix** (preflight
warns `stale_install` predates `1f220d1d` every turn). Committed-but-NOT-LIVE,
directly relevant:

| commit | fix | targets |
|--------|-----|---------|
| `96b0b47e` | preserve live queue deletions during IPC drift | the queue-strike-loss seen this session |
| `38bf22cb` | converge stale cache recovery via editor IPC | the IPC recovery path |
| `41eee413`/`69924c09`/`320a21d5` | #rtwatch rungs 1–3b | read-side base = newest-of(disk,buffer) |
| `23a3ce5b` | #kp5z drain-owner + stuck-handoff reaper | dual-drain flood + wedged Preparing controllers |
| `7b6f022e` | #w42v/#kp5z compact converge via editor IPC | compact File Cache Conflict |
| `1f220d1d` | #mps model-projected baseline (default) | the baseline mechanism itself |

Steps (pure Claude session, cwd `src/agent-doc`):
1. `make install` (rebuilds `~/.cargo/bin` binary) + `agent-doc lib-install` (refreshes the JB cdylib by mtime hot-reload).
2. `cargo build --release` (the `.bin/agent-doc` symlink target) per the release checklist.
3. Recycle stale controllers so they respawn on the fresh binary:
   `pkill -f 'agent-doc controller serve --project-root /home/brian/work/btakita/agent-loop'`
4. **Re-baseline:** restart the live JB/tmux session and run a few normal
   `/agent-doc` cycles. Record which drift signatures REMAIN in `ops.log`. Several
   of the failures this session (queue-strike loss, stale-cache recovery, stuck
   handoff, read-side base lag) are addressed by the table above and may simply
   disappear once live.

**Decision gate:** only the drift that survives Phase 0 needs Phases 1–3. Do not
implement #fintol/#pcwc against symptoms that were already fixed in committed code.

## Phase 1 — finalize tolerance for concurrent edits — #fintol

Forward-CRDT-merge an independent concurrent user edit with the response instead of
rejecting (`live_prompt_drift_after_preflight`). Disjoint from the response target
span ⇒ merge + commit the union + carry the new prompt to next cycle; collision ⇒
fail closed. Closeout-side complement to #rtwatch. Rungs #fintol1/2/3, SimWorld-
verified. Plan: `plan-finalize-tolerance.md`. **Run only if Phase 0 still shows the
reject/recovery on a benign concurrent edit.**

## Phase 2 — post-commit worktree auto-remediation — #pcwc

Detection landed (`emit_postcommit_worktree_check`, `git.rs:3336`); add auto-repair:
on `match=false` where HEAD is authoritative, reconcile the worktree to HEAD AND push
the committed content to the editor buffer, instead of warning + leaving a manual
`git checkout HEAD`. The editor-buffer push overlaps #rtwbcast. SimWorld worktree==HEAD
guard + post-commit stale-buffer-writeback case. Plan:
`plan-claudescore-3-post-commit-head-boundary-drift.md` (open auto-repair checkbox).

## Phase 3 — multi-editor / editor-buffer broadcast — #rtwbcast (durable fix)

The durable root fix for both #pcwc and the broader drift: when the file or any
editor buffer changes, broadcast the CRDT-merged content back to ALL open editor
buffers so an editor never holds stale content to write back. Blocked on an
editor-plugin protocol decision (Option A: per-editor channel + `editor_id`);
decomposed #rtwbcast-id/-deliver/-merge/-verify. Plan:
`plan-realtime-editor-watcher.md` (Rung 4 section). SimWorld two-editor coverage.

## Sequencing rationale

Phase 0 first because it is cheap and may resolve most drift (old code is live).
Phases 1→2→3 are increasing blast radius; 1 and 2 are Rust-only and SimWorld-testable
now; 3 needs the plugin protocol decision. A clean run is achievable after 0–2 for
the single-editor case; 3 hardens the multi-editor case.

## How to run (pure Claude session)

```
# NOT via /agent-doc. A plain Claude Code session:
cd /home/brian/work/btakita/agent-loop/src/agent-doc
# Phase 0 first; then per phase: edit → make check → SimWorld tests → make install
#   + lib-install + recycle controllers → commit. Verify clean-run exit criteria.
```

Each phase: full `make check`, deterministic SimWorld coverage (operator directive:
SimWorld substitutes for live-IDE verify), commit in the submodule. Do NOT drive the
`agent-doc-bugs2.md` controller from the implementing session.
