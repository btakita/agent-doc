# Plan: Response capture must include the cycle's backlog mutations

`#capturebacklogatomic` · `#agent-doc-bug`

## Symptom

When a `finalize` carrying `--backlog-add` (or other backlog) mutations captures its
response but then fails **before** the backlog mutations are recorded, the cycle
strands in `response_captured` with:

- `requires_backlog_capture = true`
- `pending_added_this_cycle = false`

Preflight interrupted-cycle recovery then cannot close it:

```
[commit] captured response body is not present in the staged snapshot ... refusing
         already-committed closeout. Replay the captured response with
         `agent-doc write --commit <FILE>` ...
[preflight] interrupted-cycle repair warning: active prompt requested backlog capture
         but no backlog mutations were recorded this cycle
Error: previous cycle `<id>` is still `response_captured` after recovery/commit
```

The suggested recovery (`agent-doc write --commit`) replays the **response only** —
which re-trips the `requires_backlog_capture` gate — so the only way out is a manual
`mv .agent-doc/state/cycles/<hash>.json` (and the capture / op-capture) aside. That
manual dead-end is a churn multiplier: recovery is not self-healing.

## Root cause

Response capture and backlog mutation are not atomic. `finalize` captures the parsed
response body (`.agent-doc/captures/<doc-hash>/<cycle-id>.json`, `response_sha256`)
as the durable replay unit, but the `--backlog-add` flags are applied later in the
same command. A failure between those two steps leaves a capture that, when replayed,
reconstructs a cycle the backlog gate correctly rejects.

## Fix

1. **Capture the whole cycle intent, not just the response.** Persist the pending
   backlog/backlog-flag mutations alongside the response body in the capture unit, so
   a replay reconstructs a *complete* cycle (response + backlog) that satisfies the
   `requires_backlog_capture` gate. (Aligns with the `#opcapturelazily` /
   `#sidecarstate` direction of one coherent captured-cycle state.)
2. **Or** apply backlog mutations *before* capturing/committing the response so the
   staged snapshot the capture validates against already contains them.
3. **Make interrupted-cycle recovery able to supersede an uncommittable capture.**
   When a captured cycle can never satisfy its own gate (e.g. `requires_backlog_capture`
   with no recorded/ capturable backlog and no live path to add it), preflight must
   discard/supersede it with a logged reason instead of erroring
   "still response_captured after recovery" and requiring a hand `mv`.

## Verification

- SimWorld: a finalize that fails after response-capture but before backlog-apply is
  recovered by the next preflight into a complete committed cycle (response + backlog),
  with no manual sidecar move.
- SimWorld: an uncommittable capture is superseded with a logged reason, not an
  infinite `response_captured` error.
- `make check` + `make tmux-ci` green.

## Related

- `#stale-lease-cpc-authority` (0.34.75) — the refusal that first stranded the cycle
  here; fixing it removes the trigger, this makes the recovery self-healing for any
  other mid-finalize failure.
- `#opcapturelazily` / `#sidecarstate` — the durable-capture-on-lazily-state direction.
