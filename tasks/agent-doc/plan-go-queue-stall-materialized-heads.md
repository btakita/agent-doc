# Plan: go-mode queue churns on pre-materialized undrainable heads (#goqstall2)

## Symptom (live-reproduced 2026-06-13, agent-doc-bugs2.md)

After `#qflood2` and `#splitmodswrite` (the genuinely drainable heads) were
drained/closed, the go-mode queue kept serving heads on every `/agent-doc`
invocation and `session-check` kept returning `queue_continuation_required=true`,
producing back-to-back no-op-class closeouts (session-accretion reached `warn`
with "3 no-op closeouts in the last 30 minutes"). Each remaining head was either:

- a `do [#id]` head pointing at a `[clean-session]` / `[operator-verify]` backlog
  item (`#rtwbcast`, `#fcc0`, `#compactdrift`) — undrainable in a live IPC session;
- a stale pasted **console-output block** that is evidence for already-done work
  (the efs `dispatch coalesced (#qflood)` block → fixed by `#qflood2`; the
  `JB Run Agent Doc … does not submit` line → `#2v7b`, code-complete last session;
  the `JB Compact Exchange` error block → `#compactdrift` evidence).

The operator had to pause the queue by hand (`admin queue pause`, receipt 25) to
stop the churn — the same manual pause this session needed once before
(receipt 3309).

## Root cause

`#goqueuestall` (shipped `6d9c4512`) only defers undrainable heads at the
**backlog → queue sync** boundary: the go-mode sync reads `[clean-session]` /
`[operator-verify]` tags on *backlog* items and skips adding them to the auto-drain
queue, and `queue_continuation_required` is computed over the drainable head set
**for sync-materialized heads**.

It does **not** cover heads that are **already materialized in the `## Queue`
block** (pre-existing `do [#id]` lines pasted/added before the fix, plus free-text
console-output lines). For those:

1. `do [#id]` queue heads are not re-resolved against their backlog item's
   `[clean-session]` / `[operator-verify]` tag, so an undrainable item still counts
   as a drainable continuation head.
2. Non-directive queue lines (pasted console output, fenced error blocks that are
   not a `do #id` / actionable directive) are treated as drainable prompts instead
   of inert noise.

So a queue pre-populated with undrainable `do [#id]` heads + stale evidence blocks
never self-defers; it churns no-op cycles until the operator pauses it.

## Fix direction (verify in a CLEAN session — touches live queue-continuation logic)

- When computing `queue_continuation_required` / the go-mode drain head set,
  resolve each `do [#id]` **queue** head against the backlog item it names and apply
  the same `[clean-session]` (under live editor-IPC) / `[operator-verify]` defer rule
  already used for backlog→queue sync. Emit `go_queue_skip_undrainable
  id=#<id> reason=<...> session=<live_ipc|clean> origin=materialized_head` so the
  defer is provable from `ops.log`.
- Classify a queue line that is neither a `do #id` head nor a recognized actionable
  directive (e.g. a fenced console-output block) as **non-drainable noise**: exclude
  it from the continuation head set and surface a one-line `queue_stale_noise_lines=N`
  diagnostic so the operator can clear them. Do **not** auto-delete queue lines (the
  live IPC supervisor races on direct queue edits).
- Net: when only undrainable/noise heads remain, `session-check` returns
  `queue_continuation_required=false queue_deferred_heads=N` with the
  "drain from a clean session / clear stale lines" note — same end-state
  `#goqueuestall` already produces for the sync path.

## Files (likely)

- `agent-doc-orchestration/src/session_check.rs` (queue_continuation_required +
  queue head classification) and `session_check/queue_head_*` guards.
- `agent-doc-orchestration/src/preflight/maintenance.rs` (go-mode sync skip;
  already emits `go_queue_skip_undrainable` for the backlog path — extend to
  materialized heads).
- Backlog-tag resolution helper shared with the `#goqueuestall` sync path.

## Coverage

- SimWorld / unit: a queue pre-populated with `do [#clean-session-id]` +
  `do [#operator-verify-id]` + a fenced console-output line, with the active editor
  IPC listener present, must compute `queue_continuation_required=false` and emit
  the per-head `go_queue_skip_undrainable … origin=materialized_head` + the
  `queue_stale_noise_lines` diagnostic.
- Regression: a genuinely drainable untagged `do [#id]` head still continues.

## Interim mitigation (applied 2026-06-13)

`agent-doc admin queue pause tasks/agent-doc/agent-doc-bugs2.md
--observed-generation 336` (receipt 25). Resume with `admin queue resume` from a
clean session once the durable defer lands, or after clearing the stale
console-output lines from the `## Queue` block.
