# Plan: preserve user-authored queue edits during convergence

## Problem

Live repro from `src/boost-client/tasks/monsterrodholders.md` on 2026-05-30:

1. The operator manually added `- do [#gscaccess]` as the first item in
   `<!-- agent:queue auto -->`.
2. During closeout and recovery work around `agent-doc write --commit`,
   `agent-doc commit`, and `reset --from-current --preserve-session`, agent-doc
   repeatedly rewrote the queue and deleted or reverted that edit.
3. The visible queue order was not treated as the newest source of user intent.
   Stale snapshot, HEAD, or route-owned queue state could win over the editor
   buffer.

Expected behavior: a user-authored queue insertion or reorder is executable
prompt intent. Agent-doc may consume the exact queue head that the current
response targeted, but it must not silently delete a newly added queue item.

Additional live repro from 2026-06-02:

1. The operator wrote new text into `agent:queue` while the previous agent-doc
   cycle was still running.
2. The cycle closeout completed, but the newly typed queue changes were absent
   afterward.
3. No fail-closed diagnostic surfaced before the stale queue body won.

This narrows the issue to the active-cycle write boundary, not only explicit
reset/repair flows. A response write, queue consume, IPC convergence, or commit
repair that starts from the pre-cycle queue body must re-check the live queue at
the final visible-write boundary. If the live queue gained user-authored prompt
lines after preflight and the current response did not consume them, the binary
must preserve those lines in the merged document/snapshot or abort with a
recovery message.

## Likely Root Causes

- Queue maintenance does not distinguish system-authored consume/halt changes
  from user-authored queue edits.
- IPC queue convergence can apply an older queue body over a newer visible queue
  body without a queue-specific epoch or hash check.
- The final response/consume/commit path can treat a stale pre-cycle queue body
  as authoritative after the user edits the queue during the active cycle.
- `reset --from-current --preserve-session` refreshes sidecars, but does not
  leave a durable "visible queue changed" marker that later write/commit paths
  must honor.
- Queue normalization may regenerate from backlog, done, or snapshot state and
  drop an explicitly queued id that looks completed, missing, or stale.

## Desired Behavior

1. Manual edits inside `agent:queue` are treated as user intent unless agent-doc
   can prove they are stale generated residue.
2. If the user adds or reorders a queue item, the next preflight either
   dispatches it, preserves it, or fails closed with a diagnostic.
3. Queue consumption removes only the exact queue head targeted by the current
   response.
4. If a completed or non-runnable id is re-added, preserve the line and warn
   that the queue item is not currently runnable instead of deleting it.
5. IPC and snapshot convergence are last-writer-aware for the queue body; a
   newer visible editor queue wins over stale queue convergence payloads.

## Implementation Outline

1. Add queue edit provenance detection:
   - compare visible queue body against the preflight baseline, snapshot, and
     HEAD queue bodies;
   - classify added or reordered `do [#id]` lines as `user_queue_edit` unless
     they were produced by the active closeout mutation;
   - record the queue body hash and edit provenance in cycle state.
2. Add a queue maintenance guard:
   - do not normalize, regenerate, or consume a user-edited queue body unless
     the current response targeted the exact head being removed;
   - block backlog-derived queue regeneration for the same cycle when a visible
     user queue edit exists.
3. Add a final visible-queue recheck before every whole-document write or queue
   consume snapshot update:
   - compare the queue body that the operation plans to write against the queue
     body currently visible on disk/editor;
   - union late user-authored prompt lines when safe;
   - otherwise fail closed before the stale queue body is committed.
4. Make IPC queue convergence source-aware:
   - include queue body hash or epoch in the IPC queue convergence payload;
   - no-op or fail closed when an older payload would overwrite a newer visible
     queue body.
5. Add fail-closed diagnostics:
   - name the exact changed queue ids, for example
     `visible queue changed since baseline: added #gscaccess`;
   - provide the recovery command when sidecar repair is needed.
6. Update session-check:
   - detect visible/snapshot/HEAD queue disagreement when the visible queue has
     a newer user edit;
   - fail before committing a deletion of that edit.

## Tests

- Fixture based on `monsterrodholders.md`: snapshot queue lacks `#gscaccess`,
  visible queue adds `#gscaccess` at the head, then preflight/write/commit runs
  with an IPC listener simulated. Assert `#gscaccess` remains in HEAD and the
  snapshot.
- Queue consume regression: a response to a different queue head consumes only
  that exact item and preserves user-added `#gscaccess`.
- Reset regression: `reset --from-current --preserve-session` on a visible queue
  with a user-added head saves the head into the session state, and a later
  `write --commit` does not revert it.
- Completed-item regression: if the user adds a completed id, agent-doc warns
  but preserves the line.
- IPC stale payload regression: an older queue convergence payload tries to
  remove a newly added head; the plugin or binary rejects the overwrite because
  the queue epoch/hash no longer matches.
- Active-cycle regression: while a response generated from baseline queue `A`
  is closing out, the live document changes to queue `A + B`; finalize/write
  preserves `B` or fails closed, and must not commit queue `A` over it.

## Likely Files

- `src/agent-doc/agent-doc-orchestration/src/preflight.rs`
- `src/agent-doc/agent-doc-orchestration/src/queue.rs`
- `src/agent-doc/agent-doc-orchestration/src/write.rs`
- `src/agent-doc/agent-doc-orchestration/src/commit.rs`
- `src/agent-doc/agent-doc-orchestration/src/ipc_socket.rs`
- JetBrains plugin queue convergence code if the epoch/hash crosses the FFI
  boundary
- SimWorld and CLI integration tests under `src/agent-doc/tests/`

## Acceptance

- A user-added `do [#gscaccess]` queue head survives write/reset/commit
  convergence unless the current response actually consumed `#gscaccess`.
- A queue line typed during an in-flight agent-doc cycle survives that cycle's
  finalize/write/commit boundary unless it is the exact line consumed by the
  response.
- Stale IPC queue convergence cannot overwrite a newer visible queue body.
- The operator receives a precise diagnostic instead of silent queue deletion.
