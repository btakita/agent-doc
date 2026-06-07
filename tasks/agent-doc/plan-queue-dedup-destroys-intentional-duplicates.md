# Plan: queue dedup destroys intentional duplicate prompts

## Symptom (reported 2026-06-05, agent-doc-bugs2 head 2)

> In the monsterrodholders.md queue, I had 2 `- deploy` items … but agent-doc
> removed the last one as an uncommitted change during the queue run. I
> intervened and re-added the last `- deploy` item before the commit.

The user intentionally queued the same command twice (deploy twice). agent-doc
silently dropped the second one mid-run.

## Root cause

`crate::queue::dedup_live_prompts` (`agent-doc-orchestration/src/queue.rs:631`),
called unconditionally every preflight in `run_queue_maintenance`
(`preflight.rs:3562`), collapses **all** live `Prompt` entries that share the
same trimmed text, keeping the first. Its doc comment asserts "Two identical
live queue prompts are never a valid state — they only appear when a divergent
IPC-buffer/snapshot CRDT/3-way merge duplicates a queue line
(`#adoc-queue-ipc-drift`)."

That assumption is false: a user can legitimately queue the same command twice
(`- deploy`, `- run tests`, …). The dedup destroys the intentional duplicate.

## Why a preflight-level tweak can't fix it

At preflight time, a **user-added** duplicate and a **merge-created** IPC-drift
duplicate are structurally identical — both render as two identical adjacent
`- <text>` lines, and both show "file has N copies, snapshot has fewer"
(the user's second `- deploy` was *uncommitted*, so snapshot-count comparison
does NOT preserve it). Provenance (user-typed vs merge-artifact) is the only
reliable discriminator, and it is lost by the time preflight runs.

Heuristics considered and rejected:
- **Snapshot-count-aware collapse** (only drop copies beyond the snapshot count):
  fails the reported case — the 2nd `- deploy` was uncommitted, so snapshot had 1.
- **Collapse only 3+ copies**: breaks "deploy 3×" and lets IPC-drift sit at 2
  copies (still re-serves/treadmills) for a cycle.
- **Adjacency**: both user duplicates and merge artifacts are adjacent.

## Proper fix (options, in preference order)

1. **Move dedup to the merge source (preferred).** Handle `#adoc-queue-ipc-drift`
   duplication in `merge.rs` / `crdt.rs` / the IPC write path, where the merge
   *knows* it is creating a duplicate of existing content vs preserving a new
   user-authored line. Tag merge-created queue lines (or compare merge inputs)
   so only the merge artifact is collapsed; user-authored duplicates pass through.
   Then remove (or hard-narrow) the blind `dedup_live_prompts` preflight pass.
2. **Provenance signal on QueuePrompt.** Carry a lightweight origin marker
   (user-edit vs merge-synthesized) through parse/merge so preflight dedup can
   collapse only merge-synthesized duplicates.
3. **Gate the preflight dedup on a proven IPC-drift state for this cycle.** Only
   run `dedup_live_prompts` when the cycle actually recorded a CRDT/IPC merge
   divergence (e.g. the `live_prompt_drift` / IPC-adoption path fired), never on
   a clean user edit. Weakest option — still heuristic — but a smaller change
   than 1/2 and would have preserved the reported case (no merge drift that cycle).

## Acceptance

- Two intentional identical live queue prompts (`- deploy` ×2), added by a user
  edit with no merge/IPC drift, survive preflight → both run, in order.
- A genuine `#adoc-queue-ipc-drift` merge duplicate is still collapsed (no
  unbounded queue growth / head treadmill regression).
- Regression tests for BOTH: intentional-duplicate-preserved AND
  merge-artifact-collapsed. Keep `dedup_live_prompts_collapses_duplicate_live_prompt`
  meaningful (or relocate it to the merge layer).

## Notes

- Same divergence-churn family as `#finalize-divergence-orphans-committed-head`
  (queue items lost/altered during a run because preflight maintenance can't
  distinguish user intent from merge artifacts). Fixing provenance once at the
  merge source addresses the class.
- Do NOT simply delete `dedup_live_prompts` — that reintroduces the
  `#adoc-queue-ipc-drift` unbounded-growth bug it was added to stop.
