---
name: project_lazily_queue_family_flavors_rs_reference
description: lazily-rs ships all 9 queue-family rows (QueueCell/TopicCell/WorkQueueCell x sync/thread-safe/async) — the shape the other 8 bindings should copy
metadata: 
  node_type: memory
  type: project
  originSessionId: a5a9e782-82c8-4457-962f-9adae81c9c8a
  modified: 2026-07-26T18:57:20.421Z
---

**Landed 2026-07-26 in lazily-rs (0.51.0).** The queue family now binds every
execution flavor: `ThreadSafeTopicCell`/`AsyncTopicCell` and
`ThreadSafeWorkQueueCell`/`AsyncWorkQueueCell` join the already-shipped queue
pair, closing `#lazilythreadsafetopiccel` and `#lazilythreadsafeworkqueu`.
Fan-out to the other bindings is still open (`#lzqfports`, `#lzqfjs`,
`#lzqfzig`).

**The shape, which is the same one [[project_lazily_reactivemap_unification_rs_reference]]
converged on:** a graph-agnostic core (`src/topic_core.rs`,
`src/work_queue_core.rs`, beside the existing `QueueStorage`) holding the pure
algebra, with **reactivity deliberately excluded**. Every mutator returns *which
readers changed* — a `Vec` of subscriber ids for the topic, a
`Transition::changed() -> ReaderChange` for the work queue — and each flavor
clears exactly that set on its own graph.

Three things are consequences, not choices:

- **Invalidate with the storage lock released.** A reader's compute takes the
  context lock then the core lock; an op that invalidated while still holding the
  core inverts the order and deadlocks. Single-threaded tests cannot see it — it
  shows up as a hang. Each thread-safe/async shell has a concurrent probe.
- **Thread-safe needs no `clear_slots`.** `batch()` already coalesces: `clear_slot`
  accumulates into `batched_slots` and `finish_batch` hands every root to one
  `clear_frontier_locked`. Async genuinely needed `AsyncContext::clear_slots` plus
  a synchronous-compute `computed`, both added earlier under `#lzqfrs`.
- **Nothing in the family is async-coloured.** A queue's `len`, a subscription
  cursor, and a lease decision all derive from storage the graph does not own, so
  the async reader kinds return plain values, not `Option` needing a settle step.
  `reap_expired` keeps taking the caller's `now` on every flavor — an owned timer
  would make redelivery unreplayable against the fixtures.

**The conformance finding worth remembering:** the four `topiccell_*.json`
fixtures were listed in lazily-rs's `KNOWN_UNCOVERED` ledger and *hand-transcribed*
into `tests/topic_conformance.rs` as Rust asserts instead. That reads as coverage
and cannot detect drift from the other bindings. `tests/queue_family_conformance.rs`
now replays all eleven fixtures against all three flavors through one
`TopicModel`/`WorkQueueModel` trait, with a nine-row ledger grepped out of `src/`
in both directions. See [[feedback_conformance_tests_drive_real_behavior_not_runner_bookkeeping]]
and [[reference_lazily_conformance_delivery_three_failure_modes]].

Formal: `lazily-formal/LazilyFormal/QueueFamilyReaderKinds.lean` proves exact
invalidation for the work-queue counts, broadcast-changes-exactly-the-connected,
per-subscriber independence, and that safe GC changes no reader's value (which is
what licenses `gc` taking no context on any flavor).
