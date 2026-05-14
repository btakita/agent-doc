# Plan: Sync orchestra stops after first task

## Bug

`sync orchestra` / sequential `agent-doc orchestrate --from-exchange` prematurely
stopped after completing the first requested task. The child run finished
`#orchmidrun`, committed and pushed its code, and finalized the session
document, but the parent orchestrator exited before launching task 2 with:

```text
Error: failed to spawn `agent-doc finalize`

Caused by:
    No such file or directory (os error 2)
```

The previous fix correctly avoided the original indefinite hang, but this run
still violated the orchestration contract: a sequential batch should either
continue to the next item or write a deterministic parent-owned interruption
response that explains why the rest of the batch is still open.

## Likely cause

The parent orchestrator attempted to run `agent-doc finalize` from a child or
post-step environment where the `agent-doc` executable was not resolvable by
PATH, despite the main harness shell being able to run it. The interruption
closeout path therefore depended on ambient PATH lookup instead of using the
current executable or a resolved binary path.

## Fix outline

1. Audit every subprocess spawn in the orchestration parent closeout path.
   Prefer `std::env::current_exe()` or the already-resolved agent-doc binary
   path for internal `agent-doc finalize`, `write`, and `session-check`
   invocations.
2. Add stderr/context to the parent error if an internal closeout spawn fails:
   include cwd, PATH presence, resolved binary path, completed step count, and
   remaining task count.
3. Ensure a failed parent interruption closeout does not silently drop the
   orchestration status. It should either complete the document write+commit or
   leave a recoverable preflight/write state with an explicit message.
4. Regression-test the exact case: sequential `--from-exchange` completes step
   1, detects a stop condition before step 2, and performs parent finalize with
   PATH intentionally cleared or missing `agent-doc`.

## Tests

- Unit: internal agent-doc command resolution does not rely on PATH.
- Integration: orchestrator parent closeout works with a sanitized PATH.
- Integration: after parent closeout, `session-check` is green and remaining
  tasks are still open.
- Regression: normal sequential orchestration still advances to later tasks
  when no stop condition occurs.

## Likely files

- `src/agent-doc/src/orchestrate.rs`
- `src/agent-doc/src/agent.rs` or internal command-spawn helpers
- `src/agent-doc/tests/run_integration.rs`
- `src/agent-doc/specs/07-orchestration-commands.md`
