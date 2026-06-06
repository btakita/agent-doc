# Plan — late IPC patch re-application duplicates the response and stalls the auto-queue (`#late-ipc-patch-duplicate-stall`)

**Backlog:** `#late-ipc-patch-duplicate-stall` (queue stall) + `#late-ipc-patch-response-uncommitted` (response looks uncommitted) in `tasks/agent-doc/agent-doc-bugs2.md`
**Observed:** 2026-06-06, live in this dogfooding session (agent-doc 0.34.0).

## Symptom (2 user-reported bugs)

1. **Queue stalled when it should not have.** After a finalize committed cleanly (HEAD `890486b54`, monsterrod head consumed, remaining 8), the `agent:queue auto` loop did not advance to the next head (`do [#free-text-head-consume-genuine-not-struck]`).
2. **The "last response was not committed."** From the operator's view the response sat uncommitted; `session-check` reported INTERRUPTED ("likely direct response patchback without agent-doc cycle").

## Root cause (observed, not theorized)

The successful finalize was `--force-disk` and committed `890486b54`, but its **boundary-reposition IPC timed out** and queued a fallback patch:
```
[commit] IPC reposition failed (non-fatal): IPC ack timeout (2s)
[commit] file IPC reposition patch queued: .agent-doc/patches/<dochash>.json
```
The route-owned supervisor's IPC listener (PID was wedged this session, see [[feedback_hung_agent_doc_write_stale_live_buffer]]) then applied a **stale earlier patch LATE** — re-injecting an OLDER draft of the same response (the exit-75 attempt's body, "CI run pending on the push") as a **second `### Re: ...(HEAD)` block** appended after the already-committed one. Result in the working tree: the response present TWICE.

That uncommitted duplicate block:
- makes `session-check` classify the doc as a direct-patchback interruption (the response "is not committed"), and
- leaves an open/interrupted cycle so the auto-queue continuation is blocked → **stall**.

`git diff HEAD` showed exactly `+17` lines = one duplicate response block; `HEAD` had the correct single final copy. Recovery this session: `git checkout HEAD -- <file>` (discard the stale duplicate) + reconcile; queue returned to 8 items, head live.

## Fix directions

- **Idempotent / generation-fenced patch application:** the IPC listener must not apply a patch whose target baseline/cycle is already superseded (the cycle already committed `890486b54`). Tag queued patches with the cycle id + baseline hash and drop a patch whose baseline no longer matches the live doc, instead of blindly appending. Relates to the IPC-duplication family (`#finalize-retry-ipc-response-duplication`, `#ipc-dup-tracking-double-merge`).
- **Reposition-only patches must not carry full response bodies:** a boundary-reposition fallback patch should reposition the boundary marker, never re-materialize a `### Re:` block. Audit what the queued reposition patch actually contained.
- **Stall recovery:** when `session-check` finds an uncommitted `### Re:` block that is byte-identical (modulo draft wording) to an already-committed response in `HEAD`, treat it as a late-IPC duplicate and reap it (discard to HEAD) instead of reporting an unrecoverable interruption — so the auto-queue self-heals rather than stalling.
- **De-wedge the listener:** a wedged/slow IPC listener (2s ack timeouts) should be detected and the doc moved to direct-disk writes for the rest of the session, rather than letting it apply patches minutes late. Relates to [[feedback_agent_doc_supervisor_self_race_finalize_block]].

## Acceptance

- A finalize whose IPC reposition times out never produces a duplicate `### Re:` block in the working tree.
- A late-applied patch whose baseline is superseded is dropped (logged), not appended.
- `session-check` self-heals a HEAD-duplicate uncommitted response instead of stalling the queue.
- Deterministic SimWorld/integration coverage for the timed-out-reposition → late-patch → duplicate path.
