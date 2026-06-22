# Plan: turn-boundary guard for reactive watch (mid-turn queue-edit churn)

Backlog: `#queediturn`
Trigger: "A recent monsterrodholders.md session had much churn after I edited the queue while a turn was running. One churn item included deleting a queue item that I added."

## Root cause (confirmed in code)

CRDT/stream documents get **reactive file-watching with zero debounce**
(`agent-doc-orchestration/src/watch.rs:656-657`, `Duration::ZERO` for `reactive_paths`).
The watch daemon is a separate process; before re-triggering a cycle it checks
`debounce::is_busy(file)` (`watch.rs:716-722`).

`is_busy` → `get_status` → `get_status_via_file` reads a cross-process status file
(`debounce.rs:920-949`). **That file is treated as stale after 30s**
(`debounce.rs:941`: `now.saturating_sub(ts) < 30_000`). The status is written by the
agent-doc **binary** during `preflight`/`finalize`, but the agent's
**response-composition phase** (between preflight and finalize) is not under binary
control and routinely exceeds 30s. So while the agent composes a response:

1. The status file goes stale (>30s) → `is_busy` returns **false**.
2. The operator edits `agent:queue` in the editor.
3. The reactive (zero-debounce) watch sees the change, sees "not busy", and re-triggers
   `agent-doc`.
4. The re-triggered cycle's queue maintenance runs against a half-edited queue and
   strikes/consumes a head the operator just added → **churn + deleted queue item**.

## Fix shape

Make the watch's busy check respect the **full agent turn**, not just the 30s binary
status window. Pick one (prefer A):

- **A. Watch consults `cycle_state` (preferred, durable).** In `watch.rs` just before
  the `is_busy` short-circuit (around `:716-722`), also treat the file as busy when
  `cycle_state` shows an open cycle (`preflight_started` / `response_captured` /
  `write_applied`, i.e. not `committed`). `cycle_state.rs` already persists this across
  the whole turn and is the source of truth for "cycle in flight." This is additive
  (more conservative re-trigger gating) and does not touch the 30s status staleness
  used elsewhere.
- B. Heartbeat the status file from the long-lived supervisor/watch path while the child
  turn is active, so it never goes stale mid-turn. More moving parts; risks masking real
  crashes.

## Scope / files

- `agent-doc-orchestration/src/watch.rs` — add the `cycle_state` open-cycle check before
  the reactive re-trigger.
- `agent-doc-orchestration/src/cycle_state.rs` — expose a `cycle_in_flight(file) -> bool`
  helper if not already present.
- Deterministic SimWorld/test coverage: editing `agent:queue` while `cycle_state` is
  `preflight_started` does NOT re-trigger a watch cycle; advancing to `committed` allows
  the next watch cycle.
- Spec alignment: `specs/07-session-tmux-commands.md` / watch spec — reactive watch
  respects open cycle_state.

## Validation

- `cargo test -p agent-doc-orchestration` (new watch/cycle_state guard test + existing).
- `make check`; `cargo install --path .`.
- Operator live test: edit `agent:queue` while an agent-doc turn is composing on a CRDT
  doc; confirm no churn / no deleted queue item, and the edit lands at the next turn
  boundary.

## Non-goals

- Not changing the 30s status staleness used for crash-recovery detection.
- Not changing CRDT reactive semantics; only gating re-trigger on open cycles.
