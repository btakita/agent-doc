# Plan: #queue-edit-lost-during-finalize-crdt

## Request

> A queue item typed WHILE `agent-doc finalize` is mid-write is transiently
> dropped from the queue, then recovers on a later preflight (observed live
> 2026-06-06). The CRDT merge of the concurrent user queue edit vs the finalize
> write should preserve the added item.

Related: `#queue-dedup-destroys-intentional-duplicates`,
`#finalize-divergence-orphans-committed-head`, `#queue-user-edit-overwrite`.

## Root cause (confirmed via a live repro on this doc, 2026-06-06 ~04:00)

The drop is **not** a CRDT data-loss bug — it is the IPC snapshot-adoption guard
deliberately refusing to absorb live prompt drift, then falling back to a
snapshot that lacks the user's concurrent add.

Mechanism (all in `agent-doc-orchestration/src/write.rs`):

1. Preflight captures `baseline`. `finalize` builds `content_ours` = baseline +
   assistant response, with **no** user edits made after the baseline.
2. The user adds a line to `agent:queue` during the finalize IPC write window.
   The IPC ack content (the live editor buffer) now contains that add, so the
   **candidate** snapshot differs from `content_ours`.
3. `ipc_snapshot_would_absorb_live_prompt_drift_after_preflight`
   (write.rs:476/8847) sees prompt-bearing content in the candidate that is not
   in `content_ours` and **blocks** adopting the candidate as the committed
   snapshot — correct intent: never silently bake a user's unanswered prompt
   into the committed snapshot.
4. The block path (write.rs ~8863-8927) logs
   `reason=live_prompt_drift_after_preflight`,
   `recovery=content_ours_snapshot_next_cycle`, records the missing lines via
   `record_dropped_queue_prompts`, and commits `content_ours` — which **lacks**
   the user's queue add. → transient drop.
5. The next preflight re-reads the live file (which still has the user's add on
   disk) and re-surfaces it → "recovers on a later preflight."

The post-commit safety net `dropped_queue_prompt_guard` (session_check.rs:460,
message at :496) detects the still-missing queued prompt and tells the user to
re-add it. That is detection, not prevention.

This is the same `live_prompt_drift_after_preflight` invariant that fired while
committing the `#autoloop-command-preemption` Phase 2a response this session
(IPC candidate 76165 vs content_ours 76144) — confirming the path is live and
the queue-add case is one instance of it.

## Why blocking-and-dropping is too blunt for queue-only drift

The guard treats ALL post-baseline prompt drift identically. But a user **queue
add** is materially different from a user **exchange prompt**:

- A new `agent:queue` line is queued *work*, not an unanswered conversational
  prompt being baked into a committed response. Preserving it in the committed
  snapshot is correct (it stays queued); it does not falsely imply the assistant
  answered anything.
- The existing isolation already exists: `dropped_queue_prompt_lines_after_content_ours`
  (write.rs:566) separates queue-only drift from exchange-prompt drift, and
  `content_ours_merged_with_disk_edits` (write.rs:8571) already CRDT-merges disk
  edits into `content_ours` via `merge::merge_contents_crdt`.

## Design (proposed — implement in a focused, clean-session cycle)

When the only post-baseline drift between the IPC candidate and `content_ours`
is **added `agent:queue` lines** (no exchange/response conflict, append-shaped),
PRESERVE them instead of blocking + dropping:

1. In the live-prompt-drift block path, classify the drift. If
   `dropped_prompt_lines_after_content_ours` is empty (no exchange-prompt drift)
   and `dropped_queue_prompt_lines_after_content_ours` is non-empty, take a
   queue-preserving branch instead of `content_ours_snapshot_next_cycle`.
2. Merge the user's added queue lines into `content_ours` (reuse
   `content_ours_merged_with_disk_edits` / `merge_contents_crdt`, restricted to
   the `agent:queue` component so the response/exchange stays exactly
   `content_ours`). Adopt the merged result as the committed snapshot.
3. Do NOT record those lines as `dropped_queue_prompts` (they are preserved), so
   `dropped_queue_prompt_guard` does not fire for them.
4. Keep the existing block-and-defer behavior for exchange-prompt drift
   (unchanged) — that case genuinely must not bake an unanswered prompt into the
   committed response.

Open questions:
- Interaction with `dedup_live_prompts` and `#queue-dedup-destroys-intentional-duplicates`:
  the preserved add must not be collapsed by dedup. Gate on proven IPC-drift
  provenance, consistent with that sibling plan.
- The queue head being consumed this cycle vs. a newly added tail item: the
  merge must strike the consumed head AND keep the new tail add (two independent
  queue mutations in one commit).

## Tests (must accompany the fix)

- SimWorld: finalize an exchange response while a concurrent `agent:queue` add
  lands in the IPC candidate; assert the committed snapshot contains BOTH the
  struck head and the new queue line, and `dropped_queue_prompt_guard` does not
  fire.
- Unit: queue-only drift classification (queue add present, no exchange-prompt
  drift) routes to the preserve branch; exchange-prompt drift still blocks.
- Regression: existing `live_prompt_drift_after_preflight` exchange-prompt block
  behavior unchanged.

## Status

- **GATED (root-caused 2026-06-06, not yet implemented).** Touches the core IPC
  `content_ours` snapshot-adoption path (`write.rs`), the most sensitive part of
  finalize. Implement in a clean session (not mid-churn) with full SimWorld
  coverage + tmux-CI review, because a regression here silently corrupts
  committed snapshots.

## Related

- `#finalize-divergence-orphans-committed-head` — the sibling symptom where the
  same drift leaves an unstruck committed head; likely a shared fix surface.
- `#queue-dedup-destroys-intentional-duplicates` —
  `tasks/agent-doc/plan-queue-dedup-destroys-intentional-duplicates.md`.
- `feedback_agent_doc_supervisor_self_race_finalize_block` (memory) — the
  recovery recipe for the transient-drop / divergence class.
