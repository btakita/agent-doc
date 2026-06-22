# Plan: `#qpausego` — `admin queue pause` accepted but doesn't halt a go-mode queue

**Status:** scoped / not started
**Filed:** 2026-06-19 (agent-doc-bugs2.md dogfooding session)
**Root cause located this session.**

## Problem

`agent-doc admin queue pause <FILE>` is accepted (durable controller receipt,
e.g. ids 68/70), but does NOT halt a `go`-mode queue: after the pause, preflight
still reports `queue_active=true` / `queue_paused=None` / `queue_trigger=auto`,
and the supervisor/auto-loop keeps re-dispatching the next head.

## Root cause (located)

The pause is stored in the **controller SQLite state DB** (`queue_controls`
table) via `state_store::upsert_queue_control_in_db`, scoped `document` or
`project`. The **controller dispatch path already honors it** — it blocks with
`failed_stage=queue_paused` by reading
`state_store::load_effective_queue_control_from_db(conn, document_id, project_scope_id)`
(returns `Some(status)` when the effective control `state != "resumed"`).

But the **preflight / in-session continuation path never consults that DB.**
`queue_continuation::detect_in_content` (`agent-doc-orchestration/src/queue_continuation.rs:81`)
computes `queue_active` / `queue_continuation_required` purely from frontmatter
(`fm.queue_active`) + the queue body. An `admin queue pause` writes the
controller DB, **not** the frontmatter, so:
- preflight stays `queue_active=true`, `queue_paused=None`, `queue_continuation_required=true`,
- the Claude in-session `/loop` (and the codex-hook `active_auto_queue_prompt`)
  keep draining,
- the only effective halt today is hand-editing `queue: stop` frontmatter /
  removing `go`, which races the live IPC supervisor.

So the gap is **preflight is blind to the controller pause receipt**, not the
controller dispatch (which already blocks).

## Fix

1. **Preflight reads the effective controller pause.** In
   `queue_continuation::detect_in_content`, after the `fm.queue_active` check,
   also consult the controller's effective queue control for this document. When
   an effective pause (`state != "resumed"`) is present, return `Ok(None)` (no
   continuation). Decide the read mechanism:
   - **Direct read-only DB open** (preferred for preflight): mirror
     `rpc::session_operator_status`'s test-path DB open
     (`open_state_db(project_root)` + `session_actor::canonical_document_id_in`),
     but as a read-only, best-effort check that no-ops when no DB / no controller
     exists (preflight must never fail because the control-plane DB is absent).
   - OR an IPC `queue_control` status request to the running controller (adds a
     round-trip + failure modes to every preflight — avoid unless the direct read
     is unsafe).
2. **Surface it in preflight.** Add `queue_paused: Option<String>` (reason) to
   `PreflightOutput`, set it from the effective control, and ensure
   `queue_continuation_required` is `false` and `queue_active` reflects the pause
   while a controller pause is active. The Claude auto-loop already stops on
   `queue_continuation_required == false`.
3. **Supervisor idle-watch.** Confirm the supervisor idle-queue auto-dispatch
   path also consults the effective control before injecting `agent-doc <FILE>`
   (the controller *dispatch* blocks, but verify the idle-watch *injection* does
   not bypass it for `go` queues). If it bypasses, gate it on the same effective
   control.

## Verification

- Unit: `detect_in_content` returns `None` when an effective document/project
  pause is present in the state DB, even with `queue_active: true` + `go`.
- Preflight emits `queue_paused=<reason>` + `queue_continuation_required=false`
  under an active pause; clears when resumed.
- Confirm `admin queue pause` → next preflight stops the loop (no re-dispatch),
  `admin queue resume` re-enables continuation.
- SimWorld: a `go` queue with an effective controller pause is not drained.

## Out of scope

The controller dispatch block itself (already correct). The
`queue: stop` frontmatter path (already works).
