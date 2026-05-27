# Plan — Fix IPC-timeout response mis-ordering in `agent-doc finalize`/`write`

| Field | Value |
|---|---|
| **Status** | Proposed — not yet implemented |
| **Severity** | High — corrupts the session document (response interleaved into a user prompt) |
| **Filed** | 2026-05-18 |
| **Source** | `#agent-doc-bug` from `src/session-share/tasks/planning-agent-pipeline.md` |

## Problem

When `agent-doc finalize` (or `write --stream`) appends an assistant response and
the IDE-plugin IPC times out, the fallback path appends the response at a
**stale boundary position**. If the user's latest prompt is multi-paragraph and
spans the stale boundary location, the response is inserted **between halves of
the prompt** — the response physically precedes part of the prompt it answers.

The cycle still reports `commit_success`, so the corruption is silent until the
next `session-check`, which then flags an "unresolved prompt-only closeout tail."

## Observed evidence (2026-05-18)

From a `finalize` on `planning-agent-pipeline.md`:

```
[write] socket IPC failed: IPC ack timeout (2s) — falling back to file IPC
[write] IPC patch written to .agent-doc/patches/<hash>.json (1 components)
[write] IPC timeout (2s) — falling back to direct write
[write] IPC timeout — response saved as patch, awaiting plugin
[main 2ed60504] agent-doc(planning-agent-pipeline): ...
[commit] IPC reposition failed (non-fatal): IPC ack timeout (2s)
```

Resulting exchange order: `[prompt part A] [assistant response] [prompt part B]`.
`agent-doc repair` afterward only "moved stale boundary to the end of the
completed exchange turn" — it corrected the boundary but did **not** reorder the
response relative to the split prompt.

## Root-cause hypotheses (to confirm during implementation)

1. **Append position is derived from the boundary marker, not document
   structure.** When IPC fails, the boundary is not repositioned to the true end
   of the latest user prompt before the response is appended, so the append
   lands at the previous cycle's boundary — which a newly-typed multi-paragraph
   prompt has straddled.
2. **Boundary reposition is IPC-coupled and treated as "non-fatal."** The
   reposition step depends on the IPC round-trip; when it times out it is
   skipped, leaving append and boundary inconsistent.
3. **The direct-write fallback is partial.** It writes the response but does not
   re-run the structural placement (append-after-last-prompt + boundary move) as
   one atomic, IPC-independent operation.

## Proposed fix (do not implement yet)

1. **Compute append position from document structure, not the boundary marker.**
   The response must be appended after the last user-prompt content within the
   exchange component (before trailing markers), regardless of IPC state.
2. **Make boundary reposition IPC-independent and atomic with the append.** The
   direct-write fallback should perform append + boundary-reposition as one
   consistent disk write; never leave a committed cycle with the boundary behind
   trailing prompt text.
3. **Post-write structural assertion.** After any write (IPC or fallback),
   verify the response is positioned after all user-prompt lines in the exchange
   tail. If not, fail closed and route to repair instead of reporting
   `commit_success`.
4. **Strengthen `agent-doc repair`.** Repair should detect the
   *response-precedes-prompt* corruption (assistant response followed by
   prompt-bearing user text in the same committed turn) and reorder the turn —
   not only reposition the boundary.

## Acceptance criteria

- A `finalize`/`write` with both socket and file IPC forced to time out still
  produces `[user prompt] [assistant response] [boundary]` in correct order.
- A multi-paragraph user prompt that straddles the prior boundary is never
  split by the appended response.
- `agent-doc repair` reorders an already-corrupted `response-precedes-prompt`
  turn into the correct order.
- A post-write structural check fails closed (no false `commit_success`) when
  the response is mis-positioned.

## Test coverage to add

- Unit/integration test simulating socket + file IPC timeout during `finalize`,
  asserting correct exchange ordering.
- Test with a multi-paragraph prompt straddling the prior boundary.
- `repair` test fed a pre-corrupted `response-precedes-prompt` document.
- Regression test asserting `session-check` is green after the simulated-timeout
  `finalize` (no deferred interruption).

## Out of scope

- Fixing the underlying IPC ack-timeout itself (separate reliability concern).
  This plan makes the **fallback** correct; IPC latency/reliability is tracked
  independently.
