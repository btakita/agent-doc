# Plan: manual queue head stalls and disappears without proof

**Pending:** `#manual-queue-head-loss`
**Status:** Detection phase IMPLEMENTED+tested 2026-06-02 (see Progress).
Remaining phases (stall-path hardening, repair recovery, dedicated provenance
diagnostic) gated on live-verify.

## Progress

- **Phase 1 — detection extension (LANDED 2026-06-02):**
  `cycle_state::observe_live_queue_heads` unions the live working-tree
  `agent:queue` directive heads into the `#queue-clear-unrun-items` removal-proof
  anchor at the `write`/`finalize` boundary (`write::run_command`, before any
  pending mutation or queue convergence). A user `do [#id]` typed into the queue
  AFTER preflight recorded `active_queue_heads` — e.g. during a stalled
  owner-pane-recursion or busy-authoritative dispatch attempt — is therefore no
  longer invisible to the removal guard, which already fails closed when a
  recorded head disappears from the committed queue while its `#id` stays open in
  `agent:backlog` and the cycle never consumed/done/gated/reaped it. Tests:
  `observe_live_queue_heads_catches_dropped_manual_head_added_after_preflight`,
  `observe_live_queue_heads_allows_manual_head_still_queued`,
  `observe_live_queue_heads_allows_manual_head_consumed_this_cycle`. `make check`
  green.
- **Remaining:** Implementation-plan points 3 (harden stalled/blocked rewrite
  paths so they preserve manual heads even without a commit) and 5 (repair
  recovery: restore a lost head from snapshot/capture/IPC sidecars or fail closed
  asking restore-vs-explicit-removal), plus the dedicated
  `queue_head_removed_without_execution_proof` diagnostic name + full queue-line
  provenance (user / generation / convergence). Live-verify the monsterrodholders
  repro once binaries are reinstalled.

## Problem

Live repro reported 2026-06-02 from
`src/boost-client/tasks/monsterrodholders.md`:

1. The backlog contained open item `#shipstationaudit`.
2. The operator added `do [#shipstationaudit]` into `agent:queue`.
3. The run stalled.
4. The queue item disappeared.
5. `#shipstationaudit` remained open in backlog, so there is no execution,
   completion, gate, or explicit user-removal proof for the queue-line removal.

Expected behavior: a user-authored queue line is executable intent. If agent-doc
cannot run it immediately because of owner-pane recursion, busy-pane routing,
missing response capture, IPC drift, or another closeout blocker, the queue line
must remain visible and runnable or the command must fail closed before removal.

## Distinction From Related Bugs

- `#queue-user-edit-overwrite` covers broad preservation of user-authored queue
  edits during convergence. This repro adds the user-visible stalled-run
  contract: a manually queued runnable id must not disappear after a failed or
  incomplete dispatch attempt.
- `#queue-clear-unrun-items` covers deletion of existing active queue heads.
  This repro is narrower: a newly inserted `do [#id]` line disappeared while
  the referenced backlog item stayed open.
- `#codex-owned-pane-auto-queue-stuck` covers same-pane recursion leaving a
  head unprocessed. This repro adds the deletion invariant after the stall.
- `#codex-final-response-not-written` and missing-response recovery can be part
  of the cause, but response recovery must not be allowed to normalize away
  runnable queue heads.

## Desired Behavior

1. A user-authored `do [#id]` queue line cannot be removed unless agent-doc has
   durable proof for the exact line.
2. Valid removal proofs are:
   - the current response consumed that exact queue head;
   - the referenced item was marked done;
   - the referenced item was gated or explicitly kept open with a new follow-up;
   - the user explicitly removed the line after the stall;
   - the line is proven stale generated residue, not user-authored input.
3. If a dispatch stalls or closeout enters recovery, the queued line remains in
   the document and the snapshot.
4. If queue state differs between live editor, working tree, snapshot, and HEAD,
   session-check names the lost queue line and the still-open referenced item.

## Implementation Plan

1. Add queue-line provenance to preflight/cycle state:
   - record queue body hash at preflight;
   - record newly observed user-authored queue lines;
   - track whether each line came from the user, queue generation, or internal
     convergence.
2. Add a removed-head proof check:
   - compare preflight queue, live queue before write, intended post-write
     queue, and HEAD after commit;
   - for every removed `do [#id]`, verify the referenced item is done/gated or
     the current response consumed that exact id.
3. Harden stalled/blocked paths:
   - owner-pane recursion guard must not rewrite queue state;
   - busy-pane route queuing must preserve manually inserted heads;
   - missing-response-body/direct-response-patchback repair must not normalize
     away user-authored queue entries.
4. Add session-check diagnostics:
   - if a queue line is absent but the matching backlog/review item is still
     open, interrupt with `queue_head_removed_without_execution_proof`;
   - include the missing line, referenced id, current item state, and recovery
     command.
5. Make repair recover rather than erase:
   - when a lost queue line is found in snapshot/capture/IPC sidecars and the id
     is still open, restore it or fail closed asking the operator to choose
     restore vs explicit removal.

## Tests

- Fixture based on `monsterrodholders.md`: backlog has open
  `#shipstationaudit`; visible queue gains `do [#shipstationaudit]`; a same-pane
  or busy-pane dispatch attempt fails/stalls. Assert the queue line remains.
- Closeout regression: a missing-response-body recovery writes a response for a
  different prompt. Assert `do [#shipstationaudit]` remains queued.
- Deletion guard regression: HEAD removes `do [#shipstationaudit]` while backlog
  keeps `#shipstationaudit` open. Assert session-check fails and names the lost
  line.
- Valid consume regression: response targets `do [#shipstationaudit]` and marks
  it done or gated. Assert only then may the queue line be removed.
- Explicit user-removal regression: a newer live queue edit removes the line and
  is classified as user-authored. Assert session-check accepts the removal.

## Acceptance

- The reported Monster Rod Holders sequence would either execute
  `#shipstationaudit`, leave `do [#shipstationaudit]` in the queue, or fail
  closed before committing the deletion.
- Agent-doc cannot silently remove a runnable manual queue head while the
  referenced item remains open.
- Recovery paths preserve or explicitly restore lost manual queue heads.
