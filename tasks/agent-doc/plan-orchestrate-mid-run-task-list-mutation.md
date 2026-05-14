# Plan: Orchestrator hangs after mid-run task-list mutation

## Bug

Sequential `agent-doc orchestrate --from-exchange` can hang after a child step
commits when the live exchange task list is edited during the run.

The observed run started with a synchronous orchestra batch, then the live
document gained another item (`#backlogorphan`) while step 1 was still running.
The child step completed and committed `#opsreadinessrace`, but the parent
orchestrator stayed attached to a sleeping `codex exec --json` worker and never
advanced to step 2. Recovery required stopping the parent process and repairing
the document closeout.

## Likely cause

The orchestrator snapshots the task batch at startup, while the child step and
normal preflight/finalize machinery continue to merge against the live document.
When the exchange list changes mid-run, the parent and child disagree about the
active prompt tail and closeout boundary:

- parent orchestration state still expects the original task sequence
- child preflight/finalize sees the newly inserted task text as live user drift
- closeout repair/prefix normalization can create a fresh prompt-bearing diff
- the parent waits for a clean child result instead of classifying the mutation
  as a deterministic orchestration interruption

The important bug is not that the user edited during orchestration. The bug is
that the binary allowed a live batch mutation to become an indefinite parent
wait instead of choosing and reporting a policy.

## Proposed policy

Freeze the orchestration batch at parent start, then detect live changes to the
source task list before each next step.

If the batch source changed during a sequential run:

1. finish any child step that already reached its closeout boundary
2. stop before launching the next step
3. write a clear exchange response explaining that the batch changed mid-run
4. leave newly added tasks open for the next explicit orchestration run
5. return a non-hanging, nonzero or soft-interrupted status with session-check
   clean

Do not silently append new tasks to the active batch in the first fix. Freezing
the batch is simpler and avoids reordering surprises around per-step commits.

## Implementation outline

1. Add a stable task-batch fingerprint in `orchestrate.rs`.
   - Capture the exact task strings and source region hash when resolving
     `--from-exchange`.
   - Store it in parent-local state and include it in step diagnostics.
2. Before advancing from step N to step N+1, recompute the source task-list
   fingerprint from the current document.
3. If the fingerprint changed, call a binary-owned interruption closeout helper
   instead of launching another child.
   - The response should name the completed step count and mention that the
     remaining/new tasks are still open.
   - The closeout must go through `finalize`/write+commit semantics and pass
     `session-check`.
4. Add a timeout/child-result guard so a child process that has already produced
   a committed closeout cannot leave the parent waiting forever.
5. Keep the behavior mode-specific: sequential orchestration gets freeze+stop
   first; parallel/DAG mutation policy can be planned separately.

## Tests

- Unit: task-batch fingerprint changes when a task is inserted, removed, or
  reordered, and stays stable for boundary-only or `(HEAD)` normalization.
- Integration: sequential orchestrate starts with two tasks, a simulated live
  edit inserts a third task while step 1 runs, step 1 commits, and the parent
  exits with a deterministic "batch changed during run" response instead of
  hanging.
- Integration: after the mutation interruption, `session-check` is green and
  the original remaining tasks plus the inserted task are still open.
- Regression: unchanged sequential orchestration still advances to step 2 after
  step 1 commits.

## Files likely affected

- `src/agent-doc/src/orchestrate.rs`
- `src/agent-doc/src/diff.rs`
- `src/agent-doc/src/session_check.rs`
- `src/agent-doc/tests/run_integration.rs`
- `src/agent-doc/specs/07-orchestration-commands.md`
