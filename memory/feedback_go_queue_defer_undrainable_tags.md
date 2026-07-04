---
name: feedback_go_queue_defer_undrainable_tags
description: "go-mode queue now defers [clean-session]/[operator-verify]-tagged backlog items instead of stalling (#goqueuestall)"
metadata: 
  node_type: memory
  type: feedback
  originSessionId: 07b4a2a4-b84c-4102-9870-3bb87525de9d
---

A `go`-mode `agent:queue` used to perpetually mirror EVERY open `agent:backlog`
item into the queue as a runnable head and assert `queue_continuation_required=true`
until drained. When every head needed a clean session or operator live-verify, the
agent correctly refused to auto-loop, so the queue never advanced — a stall where
every re-invocation re-converged the same mirrors and found nothing done (see
[[feedback_halt_flooding_queue_admin_pause]] for the older tactical halt).

**FIXED 2026-06-13 (#goqueuestall, agent-doc `6d9c4512`, superproject `eeeafeffe`,
pushed).** Two machine-readable tags now control drainability, parsed on backlog
items alongside `[recommended]`/`[TOP]`:
- `[clean-session]` — deferred when a live editor-IPC listener is active
  (`ipc_socket::is_listener_active`); drains normally in a clean (no-listener) session.
- `[operator-verify]` — always deferred (never agent-drainable; needs a human).

Go-mode backlog→queue sync skips deferred items (logs
`go_queue_skip_undrainable id=#… reason=clean_session|operator_verify session=live_ipc|clean`),
`queue_continuation` walks past deferred heads (returns `queue_continuation_required=false
queue_deferred_heads=N` + "drain from a clean session" note when only deferred heads
remain), AND `session_check`'s `check_no_response_active_queue_head` guard excludes
the deferred set (a deferred head no longer trips the "no-response reap-only closeout"
INTERRUPT). The data-loss guards (head-removal, resolution-proof) still fire — a
deferred head must STAY queued, never be dropped.

**How to apply:** in a live `/agent-doc` IPC session, tag clean-session/operator-drive
backlog items with `[clean-session]` / `[operator-verify]` (via `--pending-edit`) so
the queue defers them instead of stalling — leave genuinely agent-drainable items
(e.g. a self-contained refactor) untagged so the queue surfaces them as the real next
head. Don't blanket `queue: stop`; the tags keep the queue usable for drainable work.
Requires a binary at/after `6d9c4512` (a stale live supervisor needs a session restart;
CLI commands you run use `~/.cargo/bin` which `make install` refreshes). `--pending-edit`
flag values must avoid backticks/apostrophes (zsh substitution) — see
[[feedback_agent_doc_pending_flag_backtick_substitution]].
