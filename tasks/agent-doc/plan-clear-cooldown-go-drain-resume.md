# Plan — clear cooldown must resume an active go-mode drain (recycle/clear is a "step")

Bug id: `#clearcontresume`

## Operator-stated symptom

During an active go-mode `agent:queue` drain, a recycle + `/clear` step did NOT
resume the queue. The operator had to manually restart the queue. Operator
framing: "the recycle + clear [should be] a 'step'" inside the continuation of
the previous turn's go-mode drain — not a terminal event that stops the drain.

## Root cause (verified in code)

The **manual clear cooldown** is authoritative-until-operator-route, with no
TTL and no go-mode auto-resume:

- `session_actor_cmd::clear()` (operator `session clear` / JB Clear Exchange,
  and the deferred-operator-clear delivery) calls
  `queue_continuation::write_clear_cooldown()` after sending the clear
  (`src/session_actor_cmd.rs:543`, `:772`, `:1262`).
- `queue_continuation::clear_cooldown_active()` returns `true` as long as the
  marker file exists. It is only removed by:
  - an explicit `route::run` invocation — operator "Run Agent Doc"
    (`agent-doc-orchestration/src/route.rs:1713`), or
  - the deferred-operator-clear delivery path
    (`agent-doc-orchestration/src/start/idle_watch.rs:293`).
- The supervisor idle-queue watch consults `clear_cooldown_blocks_auto_dispatch`
  → `idle_queue_drain_decision(...)` returns `SkipClearCooldown`
  (`agent-doc-orchestration/src/start/decisions.rs:93`), suppressing passive
  dispatch **indefinitely**.
- `specs/supervisor.md:448` documents this as intended: "A manual clear cooldown
  remains authoritative and still suppresses passive dispatch until it is
  cleared by the existing operator path."

Contrast: the supervisor's OWN inter-queue-item context reset
(`IdleQueueContextResetDecision::Reset`, `start/idle_watch.rs:412`) does NOT
write a cooldown — it sends `/clear`, sets `last_dispatched = None`, and
`continue`s, so the next idle tick re-dispatches the head. That path already
treats the clear as a "step." The bug is specifically the **manual** cooldown
path leaking its indefinite suppression onto an active go-mode drain.

So: when an operator clear (or its deferred delivery) lands while a go-mode
`queue_active: true` head is pending, the indefinite cooldown stalls the drain
until the operator manually routes again — the observed bug.

## Fix direction

Make the clear cooldown a one-shot "settle" gate for an active go-mode queue,
preserving the authoritative-until-operator-route semantics for the
non-go / paused-loop case:

1. When `clear_cooldown_active` AND there is an active go-mode
   `queue_active: true` continuation head AND the cleared pane has reached a
   fresh idle harness prompt (one confirmed idle tick after the clear), the
   idle-queue watch should drop the cooldown marker and resume the drain
   (re-dispatch the next head) instead of `SkipClearCooldown` forever. The
   cooldown's real job is to avoid dispatching *into* the in-flight `/clear`;
   once the pane settles it has served its purpose.
2. Keep current behavior for non-go queues and for a paused auto-loop
   (deferred-clear) where the operator explicitly paused: those stay
   authoritative-until-operator-route.
3. Decision must stay pure/testable: thread the active-go-head + settled-idle
   signals into `idle_queue_drain_decision` (or a sibling guard) so a new
   variant (e.g. `ResumeAfterClearSettle`) is unit-testable without a live pane.

## Verification

- Deterministic decision-table unit tests for the new go-mode resume branch in
  `start/decisions.rs` (cooldown + active-go-head + settled-idle → resume;
  cooldown + non-go / not-settled → still skip).
- SimWorld regression: go-mode drain, inject a manual clear mid-drain, assert
  the drain resumes on the next idle tick WITHOUT an operator route, and that a
  non-go queue still requires the operator route.
- `make check` + tmux CI.

## Operator directive — push the pipeline into deterministic binary logic

Operator (same turn): "Make the recycle + clear deterministic binary logic
(Rust code). I believe you can have much more of this pipeline in the
deterministic binary logic." So the resume decision must NOT live in
skill/harness prose — it belongs as a pure, unit-tested branch in
`start/decisions.rs` driven by observable inputs (cooldown state, active-go
head, settled-idle signal), with the idle-queue watch acting only as the I/O
driver. Prefer moving any remaining recycle/clear/continuation sequencing that
currently relies on harness-side behavior into the supervisor binary so the
"recycle + clear is a step" guarantee is code-enforced, not convention.

## Scope notes

- `#clearcontresume` is the supervisor-side auto-resume.
- The Claude Code `/loop` lease side is adjacent but distinct: a `/clear` ends
  the `/loop` conversation, so after the supervisor clear the lease holder is
  gone; the supervisor must own resume (this plan), not a dead loop.
