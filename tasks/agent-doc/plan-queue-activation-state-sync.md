# Plan: Bidirectional realtime sync of queue-activation state (`go` marker ↔ `queue:` frontmatter) — `#qactsync`

## Problem

Queue-activation state has **two operator-editable representations**:

1. The `agent:queue` marker control token — `<!-- agent:queue priority go -->` / `start` / `stop`.
2. The `queue:` frontmatter value — `queue: start` (alias `go`) / `queue: stop`.

Today `explicit_queue_go_mode(&comp.attrs, fm.queue.as_deref())` and
`explicit_queue_stop_mode(...)` **read both** (either one being set decides go/stop),
and `frontmatter::merge_queue_state(&content, active)` writes the frontmatter — but the
two representations are **not kept in sync in the realtime system**. When the operator
edits only one (removes `go` from the marker, or flips `queue: start`→`stop`), the other
representation is left stale, and the documented workaround is to edit **both** (see the
`go`-marker-churn hazard: "remove `go` AND set `queue:stop`"). This is confusing and a
churn source — the same operator-authoritative gap as `#qeditrace`.

## Desired behavior (operator request 2026-07-01)

When the operator adds/removes `go` on `agent:queue`, **or** changes the `queue:`
frontmatter value, the realtime system syncs **both** representations to the operator's
edit, operator-authoritative (never revert the operator's change):

- Edit the marker (`go`↔removed, `start`↔`stop`) → `queue:` frontmatter converges to match.
- Edit `queue:` frontmatter → the `agent:queue` marker token converges to match.
- Both consistent after every realtime convergence pass; no churn/revert of the operator edit.

## Current code (start here)

- `agent-doc-queue/src/component_attrs.rs` — `QUEUE_ONLY_COMPONENT_ATTRS = [start, go, stop]`; marker-token semantics + the "alias `go`" note.
- `agent-doc-orchestration/src/preflight/maintenance.rs` — `explicit_queue_go_mode`, `explicit_queue_stop_mode`, and the `frontmatter::merge_queue_state(..)` write sites (≈1135, 1208, 1549, 2328–2478).
- `agent-doc-queue/src/backlog_sync.rs` — `queue_go_mode` branches (persisted-active vs go-mode).
- `frontmatter::merge_queue_state` — the frontmatter writer.

## Approach

1. Define a single canonical `QueueActivation` state (Start/Go, Stop, Unset) with a pure
   resolver from `(marker_token, fm.queue)`.
2. In the realtime convergence pass, detect **which** representation the operator just
   edited (edit provenance — reuse the operator-authoritative signal used for `#qeditrace`),
   and propagate that value to the other representation instead of letting one silently win.
3. On a genuine two-sided conflict (both edited in the same window to different values),
   pick deterministically and log; never silently revert either.
4. Keep it a fixed point: a second convergence pass over an already-synced doc is a no-op.

## Acceptance

- Editing the marker converges `queue:` frontmatter to match in the realtime system, and vice versa.
- No churn/revert of the operator's queue-activation edit across preflight/convergence.
- Convergence is a fixed point on already-synced input.
- SimWorld coverage for both edit directions + the two-sided conflict case.
- `make check` green; `make tmux-ci` + a live editor eyeball for the realtime path.

## Related

`#qeditrace` (operator-authoritative queue-edit reconcile), the `go`-marker-churn hazard,
`#mirrorall`/`#rz3a`/`#qdedupsync` queue maintenance, and the in-flight `queue and
controller policy seams` refactor (submodule `d34b622e`) — land this on top of that seam
extraction, not against a stale tree.
