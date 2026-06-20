# Plan: editor buffer wins for unconsumed user queue edits (`#editorbufwin`)

**Status:** PLAN ONLY — do not implement blind. A first implementation attempt
(2026-06-19) proved this is NOT a simple "splice the disk queue into
content_ours" change; it collides with a deliberate invariant (below). Needs a
focused clean cycle. Filed via `#agent-doc-bug`.

## Symptom (operator-reported, reproduced live this session)

The operator typed a new queue head into the live editor buffer while an
agent-doc finalize was committing. The `content_ours` IPC recovery dropped it:
`session-check` fired **INTERRUPTED** (`#queue-user-edit-overwrite`) — *"user-authored
agent:queue edit(s) were dropped during an IPC content_ours merge and are missing
from the visible document"* — and recovery required a manual
`agent-doc reset --from-current --preserve-session` → `commit`. The operator's
own framing: *"the queue reverted back to the disk state; the editor buffer should
have won as the editor buffer is the source of truth."*

Reproduced TWICE this session: every finalize hit `live_prompt_drift_after_preflight`
(`recovery=content_ours_snapshot_next_cycle`), and the operator's concurrent
`#advance-review` queue addition was dropped from the committed HEAD each time.

## Root cause (empirical — corrected from the first guess)

**First guess (WRONG):** "`content_ours` doesn't carry the user's queue addition,
so splice it in." Implementing that broke
`write::ipc::core_tests::ipc_live_prompt_drift_content_ours_ignores_unproven_live_queue_deletions`,
which **deliberately asserts** a live queue addition (`do [#manual]`) must NOT be
absorbed into `content_ours`:

> `live queue addition must not be absorbed into the response commit`

The rationale: `content_ours` is the agent's **snapshot**. If an *unanswered*
user queue head is absorbed into the snapshot, the next preflight diff
(snapshot vs disk) sees no delta and the head looks **already handled** — so the
agent could silently skip a prompt it never answered. So "additions stay
next-cycle visible on disk, not in `content_ours`" is intentional.

**Actual drop site:** the head is preserved on disk by design, but the
**post-commit worktree reconcile (`#pcwc`)** overwrites it. Sequence:
1. `finalize` IPC proof insufficient → snapshot saved from `content_ours` (no
   addition, by design). `finalize` then errors at the queue-consume step.
2. `agent-doc commit` commits `content_ours` to HEAD (no addition).
3. `postcommit_worktree_check match=false` → *"working tree lost committed
   content; auto-reconciled to HEAD via `disk_after_failed_editor_refresh`"* —
   this **overwrites the disk** with HEAD (`content_ours`), dropping the user's
   addition from the **visible document**.
4. Only the JB editor buffer re-flushing the addition back to disk saved it; the
   committed HEAD still lacked it, so `session-check` correctly flagged
   INTERRUPTED.

So the bug is the **`#pcwc` reconcile resolving disk→HEAD unconditionally** when
HEAD (`content_ours`) is missing an unconsumed user queue addition that the disk
(editor buffer) holds. The editor buffer is the source of truth there; the
reconcile should preserve the addition, not erase it.

## Invariant that must NOT regress

- An **unanswered** user queue head must never be folded into `content_ours`'s
  snapshot such that the next preflight treats it as already-handled
  (`ipc_live_prompt_drift_content_ours_ignores_unproven_live_queue_deletions`).
- An unproven candidate queue **deletion** (stale editor buffer / empty queue)
  must still be ignored (`#qdelipc`) — `content_ours` wins over deletions.

The fix must preserve the user's addition on **disk / the visible document**
(and ideally auto-recover the commit) WITHOUT absorbing it into the snapshot in a
way that hides it from next-cycle processing.

## Fix direction (to design + implement in a focused cycle)

- **P1 — `#pcwc` reconcile preserves unconsumed user queue additions.** When the
  post-commit worktree reconcile finds disk ≠ HEAD and the disk delta is an
  *unconsumed user queue addition* (present on disk, absent from HEAD, a genuine
  user `Prompt` head not struck/answered this cycle — reuse
  `dropped_queue_prompt_lines_after_content_ours`), do NOT overwrite the disk to
  HEAD. Keep the addition in the visible document (the editor buffer wins) and
  leave it for next-cycle drain. Only reconcile disk→HEAD for drift that is NOT a
  live user queue addition.
- **P2 — auto-recover the commit instead of manual `reset --from-current`.** When
  `content_ours` is committed but the disk holds an unconsumed user queue
  addition, fold that addition into a follow-up commit (the same effect the
  operator's manual `reset --from-current` → `commit` produces) so HEAD matches
  the visible document and `session-check` does not need to INTERRUPT. This is
  the "automate the recovery the operator does by hand" half.
- **P3 — keep the snapshot honest.** The snapshot may carry the addition ONLY if
  next-cycle processing still treats an unanswered head as drainable (queue drain
  reads heads regardless of snapshot diff, but the exchange-diff handled-detection
  must not mark it answered). Verify against the
  `ignores_unproven_live_queue_deletions` invariant before changing snapshot
  composition; prefer P1/P2 (disk + commit) over mutating `content_ours`.
- **P4 — supervisor CRDT staleness (root enabler).** The drift recurs because the
  route-owned supervisor's in-memory CRDT goes stale vs the live editor buffer,
  so `content_ours` keeps winning. Consider refreshing the supervisor CRDT from
  the live buffer at the idle boundary (or on `live_prompt_drift_after_preflift`)
  so the candidate isn't stale in the first place. `admin recycle` is the manual
  workaround today.

## Test coverage (SimWorld / unit, deterministic — no live tmux)

- Post-commit reconcile preserves an unconsumed user queue addition on disk (does
  not overwrite to HEAD); HEAD is then auto-reconciled to include it (P1+P2).
- The `#qdelipc` deletion-ignore invariant still holds (a candidate deletion does
  not resurrect onto disk, `content_ours` wins).
- The `ignores_unproven_live_queue_deletions` invariant still holds (an unanswered
  head is not absorbed into `content_ours` such that it reads as handled).
- End-to-end: a finalize that hits `live_prompt_drift_after_preflight` with a
  concurrent user queue addition reaches `committed` + `session-check` ok WITHOUT
  a `#queue-user-edit-overwrite` INTERRUPT and WITHOUT manual `reset --from-current`.

## Acceptance

`make check` + `make tmux-ci` green; the four invariants above hold; on the repro
shape a concurrent user queue addition survives a `content_ours` drift recovery
automatically (HEAD == visible document, no INTERRUPT, no manual reset). This is
merge/commit/post-commit-reconcile core behavior — **not** safe to rush inside an
in-session loop (a first attempt already tripped a hidden invariant); needs a
focused operator-driven cycle.

## Related

`#queue-user-edit-overwrite` (session-check detection — already fails closed),
`#qdelipc` (deletion-ignore), `#pcwc` (post-commit worktree reconcile),
`#smconv`/`#fintol2` (the existing live-prompt-drift merge paths in `write/ipc.rs`),
`#supselfheal` (supervisor recycle clears the stale CRDT that enables the drift).
