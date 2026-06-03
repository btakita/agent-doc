# Codex Stop Hook Cross-Document Queue Continuation

## Problem

The Codex Stop hook can force the current Codex session to continue a different document's `agent:queue auto` work.

Live repro, 2026-06-02:

- Current Codex session owns `tasks/software/lazily-rs.md` in pane `%74`.
- `tasks/agent-doc/agent-doc-bugs2.md` is owned by a live Claude actor in pane `%70`.
- `agent-doc hook codex-stop` found a durable queue-continuation marker for `tasks/agent-doc/agent-doc-bugs2.md`.
- The hook instructed the `%74` Codex session to run `agent-doc tasks/agent-doc/agent-doc-bugs2.md`.
- That caused this session to repeatedly service a foreign-owned document and hit pane ownership mismatch / scaffold drift recovery paths.

## Root Cause

`codex_hook.rs::marker_fallback_continuation_response` uses `queue_continuation::pending_marker_continuation_for_roots` when no tracked in-memory Stop-hook session state exists.

That fallback scans all `.agent-doc/queue-continuations/*.json` markers under the project roots and returns the first still-valid continuation. It re-confirms the marker against the live document, but it does not prove that the marker document belongs to the current Codex session or is unowned and safe to claim.

## Fix

Before returning a Stop-hook block for a durable marker fallback:

1. Resolve the marker document's authoritative actor/session ownership.
2. If the document is owned by the current Codex session, allow the continuation.
3. If the document is unowned or stale-owned, allow only through an explicit safe-claim path already used by route/session lifecycle.
4. If the document is owned by another live actor, skip the marker and log an ops diagnostic such as `codex_stop_foreign_queue_marker_skip`.
5. Continue scanning other markers instead of returning the first foreign-owned continuation.

The Stop-hook message should never instruct a Codex pane for document A to run `agent-doc <document B>` while B has a live authoritative actor.

## Tests

- Unit/integration test for `marker_fallback_continuation_response`: current Stop input has no tracked state, marker points to a document owned by another live actor, hook returns `Continue` or finds only a same-owner marker, not a `Block` for the foreign document.
- Regression for marker scan ordering: a foreign-owned marker before a current-session marker must skip the foreign marker and continue to the valid marker.
- Existing stale marker cleanup behavior must stay intact when no live continuation remains.

## Acceptance

- A Codex Stop hook in `tasks/software/lazily-rs.md` does not prompt the user to run `agent-doc tasks/agent-doc/agent-doc-bugs2.md` when `agent-doc-bugs2.md` is owned by Claude pane `%70`.
- `session-check` can still report `queue_continuation_required=true` for the foreign document, but the current Codex hook treats it as another actor's work.
- Durable queue continuation still works for the current Codex-owned document when tracked Stop-hook state is missing.
