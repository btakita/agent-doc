# Plan — backlog `queue` attribute did not populate `agent:queue` (`#backlog-queue-sync-no-populate`)

## Bug Report

**Document:** `src/boost-client/tasks/monsterrodholders.md`
**Agent:** opencode (GLM-5.1)
**Date:** 2026-06-03

### Symptom

The document has `<!-- agent:backlog queue -->` with 2 active items (`#pp-images`, `#pp-monitor`) and an empty `<!-- agent:queue auto preset="#spec-test-commit-push" -->` with `queue_active: true` in frontmatter. The backlog→queue sync should have populated the queue with `do [#pp-images]` and `do [#pp-monitor]` then auto-triggered, but neither happened.

Additionally, `do [#pp-images]. #spec-test-commit-push-deploy` appears in an HTML comment _outside_ the `agent:queue` component markers (after `<!-- /agent:queue -->` and a `###` heading), suggesting either a manual edit placed it there or a prior population attempt wrote outside the component boundary.

### Root Cause Analysis

The backlog→queue sync is implemented in `preflight.rs` `run_queue_maintenance` (line 3313), which calls `collect_backlog_queue_sync` (line 3268). This function:

1. Scans `agent:backlog` / `agent:icebox` components for a `queue` attribute
2. Parses the value via `BacklogQueueSyncMode::parse` (empty string → `Append`)
3. Calls `sync_backlog_into_queue` to regenerate queue entries
4. Writes the mutated document back to disk

**Root cause: the sync only fires during binary preflight (`run_queue_maintenance`), which is invoked by the agent-doc binary during route/dispatch cycles.** In an opencode session without the binary running a preflight pass, the declarative `queue` attribute on `agent:backlog` is inert — no code reads it outside the binary preflight path.

**Secondary finding:** the `do [#pp-images]. #spec-test-commit-push-deploy` text outside the queue markers (lines 44-47) is in a plain HTML comment after `###`, not inside `<!-- agent:queue -->...<!-- /agent:queue -->`. This text is never processed as a queue entry because it is not between the component boundaries.

### Evidence

- `parse_attrs` (component.rs:577) correctly parses bare `queue` → `attrs["queue"] = ""`
- `BacklogQueueSyncMode::parse("")` returns `Some(Append)` (queue.rs:374)
- `collect_backlog_queue_sync` correctly reads active item ids from the backlog body
- The sync path IS implemented and tested — it works when the binary preflight runs
- The `preset` attribute on `agent:queue` is orthogonal and does not block sync

### Possible Fixes

**Option A — Document the limitation and provide a manual trigger.**
- Add a preflight warning when `agent:backlog` has a `queue` attribute but the session is opencode/direct (no binary preflight).
- Provide a `agent-doc queue sync` (or `--queue-sync`) CLI command the operator can run manually to trigger one-shot sync.
- The plan file `plan-backlog-queue-sync-attribute.md` already proposes a one-shot, provenance-aware model. The missing piece is that the one-shot needs a manual trigger when the binary preflight is not in the loop.

**Option B — Make the LLM harness read and honor the `queue` attribute.**
- When the agent-doc harness (opencode, Claude, Codex) reads the document for dispatch, parse `agent:backlog queue` and populate `agent:queue` in the read/edit phase before dispatching the first prompt.
- Risk: this duplicates binary logic in the harness prompt and may conflict with binary preflight when both run.

**Option C (recommended) — Option A + preflight diagnostic.**
- Emit a `backlog_queue_sync_pending` preflight warning when `agent:backlog` carries `queue` and the queue is empty (or has fewer entries than the backlog).
- The warning names the expected action: "run `agent-doc queue sync` to populate the queue from backlog items, or the binary will sync on the next route/dispatch cycle."
- When the binary DOES run preflight, the sync fires automatically as it does today — no change to the existing path.
- Document the opencode limitation: `agent:backlog queue` requires binary preflight or manual `agent-doc queue sync`.

### Tests

- Preflight emits `backlog_queue_sync_pending` warning when `agent:backlog` has `queue` attr and queue is empty/undersynced.
- `agent-doc queue sync` (or `--queue-sync`) CLI flag triggers one-shot sync from the first queue-tagged backlog component.
- Warning is NOT emitted when the queue already matches the backlog (idempotent).
- Warning is NOT emitted when `agent:backlog` has no `queue` attribute.
- Existing `sync_backlog_into_queue` tests continue to pass unchanged.

### Non-goals

- Do not change the binary preflight sync behavior — it works correctly when the binary runs.
- Do not implement Option B (LLM-side sync) — too much duplication and conflict risk.
- Do not address the misplaced `do [#pp-images]` outside the queue markers — that is an operator edit artifact, not a bug.

### Related

- `#backlog-queue-sync-attribute` — the original feature plan
- `#backlog-priority-attribute` — priority ordering, composable with sync
- `#queue-sync-auto-pending-done-guard-misfire` — guard against false-closed on synced ids
- `plan-backlog-queue-sync-attribute.md` — design doc for the sync feature
