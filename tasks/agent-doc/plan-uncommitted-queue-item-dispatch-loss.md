# Plan: `#qdispatchloss` — uncommitted queue item consumed + lost on JB dispatch

**Status:** IMPLEMENTED 2026-06-19 — Option A (fail-closed) shipped in `route.rs`.
`inactive_route_queue_head_in_content` now proves the candidate inactive queue
head is backed by the committed snapshot (`route_queue_head_backed_by_committed_snapshot`
→ `snapshot::load`) before surfacing it; an unbacked head defers (returns `None`,
logs `route_dispatch_uncommitted_head ... reason=head_not_in_committed_snapshot
decision=defer`). Coverage: `route_defers_uncommitted_queue_head_not_in_committed_snapshot`,
`route_dispatches_committed_queue_head`,
`route_queue_head_unbacked_when_committed_snapshot_has_no_queue`,
`route_queue_head_backed_allows_when_no_committed_snapshot`. SPEC
(`08-session-routing.md`) + VERSIONS + FlowCore budget (`route.rs` `reason=` 8→9)
updated. `make check` green (4745 passed). The shared durable-journal primitive
with `#qdurcrash` (Option A's "commit the inbound queue edit first" variant) is
deferred to `#qdurcrash`; this fix achieves the same safety by deferring the
dispatch to the normal preflight-commit path instead of feeding an uncommitted
line as the prompt.
**Filed:** 2026-06-19 (agent-doc-bugs2.md dogfooding session)
**Siblings:** `#qdurcrash` (queue-edit durability across a crash+restart),
`#1vhn`/`#routedeadshell` (dispatch into a crashed harness), `#semmerge`
(live-merge drop). This is the **no-crash** counterpart of `#qdurcrash`: the
supervisor never dies, but a dispatch consumes an operator queue edit that was
only ever in the live editor buffer and never committed.

## Problem (operator repro)

> JB `Run Agent Doc` in equityfundingsource.md moves an uncommitted queue item
> from the queue into the agent prompt. The queue item is gone and the turn
> stalls uncommitted with incorrect agent input.

The operator types a new queue item into the editor buffer and, before it is
committed (it exists only in the IntelliJ buffer / synced to disk by the plugin
but not yet in a git-committed snapshot), triggers `Run Agent Doc`. The route
dispatch path:

1. reads the live document from disk (`route.rs` selects the queue head via
   `std::fs::read_to_string(file)` — e.g. `inactive_route_queue_head`
   `route.rs:2618`, `activate_existing_route_queue_head` `route.rs:2670`, the
   active-head read at `route.rs:2291`),
2. picks that uncommitted item as the dispatch prompt and "moves it into the
   agent prompt" (possibly a half-typed / wrong text — "incorrect agent input"),
3. consumes/activates the head, but the consumption never reaches a committed
   snapshot, so the item is **gone** from the queue and the turn **stalls
   uncommitted**.

## Why it happens (hypothesis — to confirm)

Route dispatch treats the on-disk queue head as authoritative and consumable
without proving it is backed by a committed snapshot. An operator edit that is
only in the editor buffer (or freshly synced to disk by the plugin but not
git-committed) has no durable backing, so:

- the dispatched prompt may be a partially-typed line (the operator was still
  editing) — "incorrect agent input";
- activating/striking the head mutates only the uncommitted working state, and
  the subsequent IPC/CRDT drift (`#semmerge` content_ours adoption) drops the
  strike, so the item disappears with no committed answer.

This is the dispatch-time analogue of `#qdurcrash`'s "queue edit lived only in
the editor buffer / in-memory CRDT and was never journaled before it was lost."

## Fix direction (to design)

Make route dispatch refuse to **consume** a queue head that is not yet backed by
a committed snapshot, OR durably journal the operator queue edit before
dispatch:

- **Option A (fail-closed):** before selecting/consuming a queue head, verify the
  head is present in the committed snapshot (not just the live disk buffer). If
  the next head is an uncommitted editor-buffer edit, commit the queue edit first
  (binary-owned snapshot/journal of the inbound queue patch, shared with
  `#qdurcrash`) and only then dispatch — never feed an uncommitted, possibly
  half-typed line as the prompt.
- **Option B (debounce):** treat a queue head that differs from the committed
  snapshot as "still being edited" and defer dispatch one debounce window /
  until the editor buffer settles + the queue edit commits, instead of racing
  the operator's keystrokes.
- Add a `route_dispatch_uncommitted_head_*` ops.log diagnostic (head text,
  committed-vs-live divergence, decision) so the live-gate can prove/disprove.

## Verification

- SimWorld/integration: operator adds an uncommitted queue head (live buffer
  diverges from the committed snapshot), `Run Agent Doc` dispatches → assert the
  head is NOT consumed-and-lost (either committed-then-dispatched, or deferred),
  and the dispatched prompt is never a partial/uncommitted line.
- Reuse the route/queue harnesses in the orchestration crate tests.

## Out of scope

Crash+restart durability (`#qdurcrash`), live-merge drop during an in-flight turn
(`#semmerge`), dispatch into a dead shell (`#1vhn`).
