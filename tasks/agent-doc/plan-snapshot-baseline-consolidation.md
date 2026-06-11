# Plan: consolidate snapshot + baseline files into git HEAD + the document model (#snbc)

Operator question (agent-doc-bugs2.md): "Do we still need separate editor snapshot +
baseline files now that we moved to the document model? The snapshot causes many conflicts
on realtime documents, plus churn and inaccurate state. Can we use the actual document +
editor buffers as the source of truth & the document model?"

**Run in a CLEAN session (cwd `src/agent-doc`), NOT via `/agent-doc` on this doc** — it
touches the closeout/merge path that this doc's controller races.

---

## Diagnosis (why the files cause drift)

A 3-way / CRDT merge is only correct if the **base** is the true common ancestor:
`base = baseline/snapshot`, `ours = editor buffer (content_ours)`, `theirs = agent patch`.

- **snapshot** (`.agent-doc/snapshots/<hash>.md`) — last-committed doc copy; preflight diffs
  the live doc against it; also the on-disk recovery source (`content_ours snapshot`).
- **baseline** (`.agent-doc/baselines/<hash>.md`) — preflight's stable post-commit copy,
  handed to `finalize --baseline-file` as the merge base.

The #rtwatch root cause: the base is read from **disk**, but the authoritative content is the
**live IDEA buffer** (unsaved, content_ours over IPC). When the buffer has unsaved edits the
base lags → the merge mis-attributes user edits → File Cache Conflict, churn, postcommit
worktree drift. The defect is not "we keep a base"; it is "the base is a separately-persisted
file read from disk instead of `newest-of(disk, buffer)`".

## Target architecture

- **base = git HEAD** — already the durable last-committed state; no separate snapshot file to
  drift. (git persists committed state across controller restart for free.)
- **current = `realtime_model::current_doc(disk, buffer)`** (#rtwwire) — no separate baseline
  file; the model is authoritative "current".
- **merge = CRDT** over (git HEAD, realtime current, agent patch).

This removes the two drift-prone files and makes git + buffers + the document model the single
source of truth — extending #rtwwire/#rtwfeed and killing the stale-snapshot failure mode.

## Caveat — audit before deleting (this is a migration, not a one-line delete)

The snapshot/baseline also back non-merge paths that git HEAD does NOT cover (HEAD is committed
state only; these hold *uncommitted in-flight cycle* state):
- recovery sidecars / `content_ours_snapshot_next_cycle`
- `reset --from-current` snapshot/CRDT rebuild
- hash-keyed cycle state (`.agent-doc/state/cycles`, captures)

Audit every read site of `snapshots/` and `baselines/` (tsift: `explain` / `graph --callers`
on the snapshot+baseline accessors) and re-home each to either git HEAD (committed base) or the
CRDT/realtime model (uncommitted current) before removing the files.

## Slices

1. **Audit + map** every snapshot/baseline read site → classify as committed-base (→ git HEAD)
   or in-flight-current (→ realtime model / CRDT). Output: a table, no behavior change.
2. **Base = git HEAD.** Replace snapshot-as-merge-base reads with git HEAD reads; keep the
   merge 3-way contract. Unit + SimWorld coverage for the realtime stale-base regression class.
3. **Current = realtime model.** Replace baseline-as-current reads with
   `realtime_model::current_doc`; verify edit-during-finalize no longer mis-attributes.
4. **Remove the files** once slices 1-3 prove no remaining readers; delete writers + dirs.
5. Build + install + lib-install; live edit-during-finalize zero-drift verify.

## Related

`#rtwatch` / `#rtwfeed` / `#rtwwire` (realtime editor watcher) — this is the same root cause
viewed from the persistence side. `#ipcdrift` / `#postcommit-ipc-worktree-corruption`.
Also addresses the "Finalize should be tolerant of unrelated user edits" queue ask — a
correct `newest-of(disk, buffer)` base stops treating concurrent user edits as conflicts.
