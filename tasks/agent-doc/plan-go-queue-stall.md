# Plan — go-mode queue stalls on agent-undrainable heads (#goqueuestall)

## Symptom

A `go`-mode `agent:queue` continuously mirrors every open `agent:backlog` item
into the queue as a runnable head and reports `queue_continuation_required=true`
until the heads drain. When every head is **not agent-drainable in the current
session** — e.g. items whose prose says "CLEAN session required" (editing from a
live editor-IPC session corrupts the closeout) or "operator-drive live verify"
(agent cannot reproduce a live JB/TUI) — the agent correctly refuses to
auto-loop, so the queue never advances. Every re-invocation re-converges the same
mirror lines, finds no completion, and re-asserts `queue_continuation_required`.
The session "stalls": perpetual phantom work, no progress.

## Root cause

The "not agent-drainable in this context" constraint lives only in **prose** in
the backlog item text. The queue state machine has no machine-readable notion of
it, so go-mode mirrors the item into the active queue and `session_check`
asserts `queue_continuation_required` regardless of whether an agent can actually
run it in the current session type (live editor-IPC vs clean).

## Fix — machine-readable execution-context attribute that go-mode respects

### 1. Parseable item attributes (queue.rs item-tag parser)
Add two author-controlled tags, parsed alongside the existing `[recommended]` /
`[TOP]` markers on `agent:backlog` items:
- `[clean-session]` — must run from a session WITHOUT a live editor-IPC listener
  (a "clean" session); running it under a live IPC listener risks closeout
  corruption.
- `[operator-verify]` — completion requires operator-driven live verification the
  agent cannot perform (no auto-drain possible at all).

These are surfaced on the parsed item model (e.g. `QueueEntry` /
backlog-item struct) as booleans `clean_session_required` /
`operator_verify_required`.

### 2. Live-IPC session detection (reuse existing signal)
Reuse the existing **listener-active** gate (the IDE editor-IPC listener detection
already used to route writes editor-IPC-vs-disk). Expose a single predicate
`session_is_live_ipc()` (listener active on the document's ipc socket). Do NOT
invent a new detector.

### 3. Go-mode backlog → queue sync skips undrainable heads
In the go-mode `synced backlog → queue` convergence (the
`#backlog-queue-attr-populates-in-go-mode` / continuous-backlog-loop path):
- When `session_is_live_ipc()`, SKIP `[clean-session]` items — do not mirror them
  into the active queue.
- ALWAYS skip `[operator-verify]` items from the auto-drain queue (they need a
  human; they belong in the gated review surface, not the runnable queue).
- Emit an observable diagnostic per skip:
  `go_queue_skip_undrainable id=#<id> reason=clean_session|operator_verify session=live_ipc|clean`
  so the behavior is provable from ops.log and so a clean session shows the items
  re-appearing.

### 4. `queue_continuation_required` excludes skipped heads (session_check.rs)
`queue_continuation_required` must be computed over the **drainable** head set
only. If the only remaining heads are skipped (`clean-session` under live IPC, or
`operator-verify`), continuation is NOT required — `session_check` returns OK with
`queue_continuation_required=false` and a one-line note
(`queue idle: N head(s) deferred (clean-session/operator-verify) — drain from a
clean session`). This is what stops the stall.

### 5. Clean session still drains them
When `session_is_live_ipc()` is false (clean session), `[clean-session]` items
mirror and drain normally. `[operator-verify]` items still don't auto-drain but
are reported as awaiting operator verification (not as a stall).

## Migration (this repo)
Tag the existing un-drainable bugs2.md backlog items so the new logic recognizes
them: `#fcc0`, `#rtwbcast`, `#compactdrift` → `[clean-session]`; `#qflood2` →
`[operator-verify]` (live multi-process). `#splitmodswrite` stays plain (it is
drainable). This is done from the session-doc cycle via `--pending-edit`.

## Tests (deterministic; queue.rs / session_check.rs `#[cfg(test)]` + SimWorld)
- Parser: `[clean-session]` / `[operator-verify]` set the booleans; plain items
  don't; combine with `[recommended]`/`[TOP]`.
- Go-mode sync under live-IPC: a `[clean-session]` head is skipped (not queued)
  and emits `go_queue_skip_undrainable reason=clean_session`; under a clean
  session it IS queued.
- Go-mode sync: `[operator-verify]` head never auto-queues regardless of session.
- `queue_continuation_required` = false when the only heads are deferred; = true
  when a drainable head (`#splitmodswrite`-like) remains.
- Regression: a normal go-mode queue with drainable heads is unchanged.

## Verification
`make check` (0 FAIL) + clippy clean; `cargo build --release` + `cargo install
--path .` + `agent-doc lib-install`; recycle. Live confirmation: in this live IPC
session, `session-check` reports `queue_continuation_required=false` with the
deferred-heads note instead of `next_queue_prompt=#fcc0`.
