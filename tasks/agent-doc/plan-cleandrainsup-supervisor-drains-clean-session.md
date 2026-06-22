# Plan: supervisor self-drives `[clean-session]` queue heads (#cleandrainsup)

## Problem

The go-mode auto-loop permanently stalls when the IDE is open. Root cause is a
deadlock in the drainability classification (`agent-doc-orchestration/src/queue_continuation.rs`):

- `deferred_backlog_ids()` defers a head when
  `operator_verify_required || (clean_session_required && live_ipc)`.
- **Both** consumers share that set:
  - the Claude Code `/loop` via `drainable_head_count()` → `queue_continuation_required`
  - the supervisor idle-watch via `idle_watch_active_queue_head()` → `live_drainable_continuation_head()`
- With the IDE open (`live_ipc == true`), every `[clean-session]` head is deferred
  in **both** paths → the loop stops AND the supervisor's `active_head` is `None`,
  so nobody ever drains them.

The SKILL claims the supervisor "owns the `/clear` and re-dispatches the next item
to a freshly-`/clear`ed agent" — but the supervisor's dispatch gate defers exactly
those heads, so that never happens.

## Fix (operator chose: supervisor self-drives them)

1. **Scope the deferred set** (`queue_continuation.rs`): introduce `DrainScope`.
   - `DrainScope::Loop` (in-session, accretes, cannot self-clean): defers
     `[operator-verify]` always + `[clean-session]` under `live_ipc` — unchanged.
   - `DrainScope::Supervisor` (force-`/clear`s + re-dispatches to a fresh agent):
     defers `[operator-verify]` only; `[clean-session]` is drainable.
   - `live_drainable_continuation_head()` uses `DrainScope::Supervisor`.
   - `drainable_head_count()` / `deferred_head_count()` / `detect()` / session_check
     stay `DrainScope::Loop` (the `/loop` still stops on clean-session, letting the
     supervisor drive without drain-owner-lease contention).

2. **Force `/clear` before a clean-session head** (`start/idle_watch.rs`): when the
   active head is a `[clean-session]` item, the idle-watch sets a context-reset
   reason independent of the `agent_doc_queue_context_reset` opt-in (still
   suppressed during a clear cooldown). The existing Reset path then `/clear`s, waits
   for settle, and dispatches the head to the freshly-cleared agent.
   - New pure decision `clean_session_head_forces_context_reset()` in
     `start/decisions.rs` (unit-testable).
   - New helpers `clean_session_backlog_ids()` / `head_requires_clean_session()` in
     `queue_continuation.rs`.

3. **Docs**: update `CLAUDE_QUEUE_AUTO_LOOP_SECTION` in `src/skill.rs` (and the
   bundled/installed mirrors via `agent-doc skill install`) so the `/loop` stop on
   `queue_continuation_required == false` for clean-session heads is documented as
   "the supervisor force-`/clear`s and drains these," not a dead no-op.

## Tests

- `deferred_backlog_ids_with_ipc_scoped`: Supervisor scope does NOT defer
  clean-session even under `live_ipc`; Loop scope does.
- `live_drainable_continuation_head` returns the clean-session head while
  `drainable_head_count` returns 0, under `live_ipc`.
- `clean_session_head_forces_context_reset` truth table.
- `head_requires_clean_session` id mapping.

## Verification

`make check`, `cargo build --release`, `cargo install --path .`, `agent-doc lib-install`,
restart this session's supervisor, confirm `ops.log` shows the supervisor
`idle_queue_watch_context_reset reason=#cleandrainsup` then a drain of the head.
