# Plan: consolidate snapshot + baseline files into git HEAD + the document model (#snbc)

Operator question (agent-doc-bugs2.md): "Do we still need separate editor snapshot +
baseline files now that we moved to the document model? The snapshot causes many conflicts
on realtime documents, plus churn and inaccurate state. Can we use the actual document +
editor buffers as the source of truth & the document model?"

**Run in a CLEAN session (cwd `src/agent-doc`), NOT via `/agent-doc` on this doc** — it
touches the closeout/merge path that this doc's controller races.

---

## Status — Slice 1 audit complete; slices 2–4 superseded (2026-06-11)

**The audit invalidated the plan's original premise.** The short answer to the operator's
question: the *drift* the snapshot/baseline caused is **already fixed** by the landed
realtime-model rungs (`#rtwatch` / `#rtwfeed` / `#rtwwire`, commits 41eee413 / 69924c09 /
320a21d5), and the two files **cannot** be replaced by git HEAD without regressing merge
correctness. See "Audit results" and "Revised architecture" below. The original
"base = git HEAD / remove both files" slices are kept at the bottom for history, struck
through, but must **not** be executed as written.

---

## Diagnosis (why the files caused drift) — still accurate

A 3-way / CRDT merge is only correct if the **base** is the true common ancestor:
`base = baseline/snapshot`, `ours = editor buffer (content_ours)`, `theirs = agent patch`.

- **snapshot** (`.agent-doc/snapshots/<hash>.md`) — last-committed doc copy; preflight diffs
  the live doc against it; also the on-disk recovery source (`content_ours snapshot`).
- **baseline** (`.agent-doc/baselines/<hash>.md`) — preflight's stable post-commit copy,
  handed to `finalize --baseline-file` as the merge base.

The #rtwatch root cause: the base was read from **disk**, but the authoritative content is
the **live IDEA buffer** (unsaved, content_ours over IPC). When the buffer has unsaved edits
the base lagged → the merge mis-attributed user edits → File Cache Conflict, churn,
postcommit worktree drift. The defect was never "we keep a base"; it was "the base is read
from disk instead of `newest-of(disk, buffer)`".

**This defect is already resolved.** `preflight` now saves the baseline as
`diff_result_with_current.current` (`preflight.rs:3196`), and `diff::compute_with_current`
sources that `current` through `crate::realtime_model::resolve_current_doc(file, &disk)`
(`preflight.rs:2675`). The same `resolve_current_doc` seam is wired into `write.rs:8429` and
`session_check.rs:262`. So the merge base is already `newest-of(disk, editor buffer)`, not
bare disk — the stale-base failure mode the plan set out to kill is closed.

## Audit results (Slice 1 — completed)

Every read/write site of the plain snapshot (`snapshots/<hash>.md`) and baseline
(`baselines/<hash>.md`) files, classified by role and re-home target. CRDT
(`.agent-doc/crdt/`) and pre-response sidecars are out of scope.

| Site | Op | Role | Re-home |
|------|----|------|---------|
| `write.rs:1091` `read_explicit_baseline` | READ | merge-base (explicit `--baseline-file`, snapshot `baseline_path_for` fallback) | stays (current-at-preflight; see below) |
| `write.rs` `template_patch_application_base` (~8198) | READ | merge-base; **already rebases to `git::show_head` when the explicit baseline is stale/missing committed exchange** | git HEAD (conditional, already done) |
| `write.rs:8655` `merge::merge_contents(base, ours, current)` | USE | the 3-way merge itself | base = current-at-preflight |
| `preflight.rs:3196` `save_baseline_content(file, diff.current)` | WRITE | merge-base prep — saves `resolve_current_doc` projection | stays (NOT HEAD) |
| `preflight.rs` `compute_diff_with_current` (963/1003/3293/6464) | READ | diff-against-baseline (user-edit display) | snapshot (in-flight) |
| `write.rs` `run_inline`/`compute_pending_state`/`run_ipc` (1413/3654/4062/4217/5636/5683) | READ | content_ours / in-flight merge state | snapshot (in-flight) |
| `write.rs` `run_stream`/`run_template` save sites (2030/10039/10083/10167) | WRITE | next-cycle baseline after response | snapshot (in-flight) |
| `capture.rs` (220/281/477/490/732) | READ | response-replay hash validation | snapshot (in-flight) |
| `repair.rs`, `preflight.rs` repair fns, `session_check.rs` recovery | R/W | orphaned/in-flight cycle recovery | snapshot (in-flight) |
| `git.rs` commit/squash (1505–3310) | R/W | post-commit snapshot↔HEAD sync | snapshot then HEAD |
| `snapshot.rs:266` `resolve` | READ | merge-base recovery fallback (git HEAD already the fallback) | git HEAD (already) |
| `snapshot.rs` `ensure_snapshot` / `try_migrate_renamed` | W | init / rename migration | snapshot (must keep) |

**Three findings that block the original plan:**

1. **The merge base is NOT git HEAD — it is "current-at-preflight".** `preflight` step 2
   (`preflight.rs:2960`) commits the *previous* cycle, but per the "selective commit is
   conservative" rule the **new user prompt stays uncommitted**. Step 4 then saves the
   baseline as `resolve_current_doc(current)` = **HEAD + the uncommitted new user prompt the
   agent is responding to**. Substituting git HEAD as the merge base would drop that prompt
   from the common ancestor and mis-attribute it as a concurrent `theirs` insertion —
   corrupting exactly the merges this work protects. git HEAD only coincides with the
   baseline in the degenerate no-new-prompt case; `write.rs`'s `template_patch_application_base`
   already uses `git::show_head` as the *narrow* HEAD-rebase fallback for the stale-baseline
   case, which is the only place HEAD is the correct base.

2. **The drift is already fixed** by the realtime-model rungs (see Diagnosis) — the base is
   already `newest-of(disk, buffer)`. There is no remaining stale-base defect to migrate away.

3. **The snapshot file cannot be removed.** Beyond the merge base it backs uncommitted
   in-flight cycle state git HEAD does not cover: `content_ours`, capture-replay hash
   validation (`capture.rs`), over-application repair, `reset --from-current`, and the
   next-cycle baseline persisted before commit. These are "uncommitted current", not
   "committed base", so HEAD is structurally unable to represent them.

`git::show_head` (`git.rs:3559`, `pub fn show_head(file) -> Result<Option<String>>`) is the
committed-base accessor and is already used for the one legitimate HEAD-as-base case.

## Revised architecture (what is actually true / desired)

- **Committed base = git HEAD** — already the recovery fallback (`snapshot.rs` `resolve`,
  `write.rs` `template_patch_application_base`). No change needed.
- **Merge base = current-at-preflight** = `resolve_current_doc(newest-of(disk, buffer))`,
  persisted for the cycle. This **must** stay a persisted artifact because it carries
  uncommitted user content (the new prompt) that HEAD cannot hold. It is correctly no longer
  "bare disk".
- **Uncommitted in-flight current = the snapshot store + realtime model + CRDT** — unchanged.

The operator's concern ("snapshot causes conflicts on realtime docs, churn, inaccurate
state") is the stale-base drift, and that is resolved. The files are not redundant with git
HEAD; they hold uncommitted state HEAD cannot represent.

## Revised slices (optional future work, lower priority)

1. ~~Audit + map.~~ **Done** (table above).
2. **Optional file consolidation (cosmetic, not correctness):** the baseline file duplicates
   a projection the snapshot store could derive. Collapsing `baselines/<hash>.md` into a
   snapshot-store variant would remove one of two files **without** changing merge-base
   semantics (still current-at-preflight, still not HEAD). Moderate risk; gate behind new
   SimWorld coverage for the prompt-bearing finalize-merge class before touching the
   closeout path. Not required to fix any live bug.
3. **No-op:** "base = git HEAD" is already correctly scoped to the stale-baseline rebase
   fallback only. Do not broaden it.

## Related

`#rtwatch` / `#rtwfeed` / `#rtwwire` (realtime editor watcher) — the persistence-side view of
the same root cause, now landed. `#ipcdrift` / `#postcommit-ipc-worktree-corruption`.
The "Finalize should be tolerant of unrelated user edits" queue ask is satisfied by the
`newest-of(disk, buffer)` base, which already stops treating concurrent user edits as
conflicts.

---

## Original slices (SUPERSEDED — do not execute as written)

> Kept for history. The audit above shows these regress merge correctness: the merge base
> must carry the uncommitted new user prompt, so it cannot be git HEAD, and the snapshot file
> holds in-flight state HEAD cannot represent.

> 1. ~~Audit + map every snapshot/baseline read site.~~ (done — see Audit results)
> 2. ~~**Base = git HEAD.** Replace snapshot-as-merge-base reads with git HEAD reads.~~
>    Unsafe: base ≠ HEAD for prompt-bearing cycles.
> 3. ~~**Current = realtime model.**~~ Already landed (rungs 1–3b).
> 4. ~~**Remove the files.**~~ Unsafe: snapshot backs in-flight state HEAD cannot cover.
> 5. ~~Build + install + lib-install; live edit-during-finalize zero-drift verify.~~
