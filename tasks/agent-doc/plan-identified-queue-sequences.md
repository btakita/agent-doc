# Identified queue sequences

Status: post-v0.35.480 implementation. This branch must remain unmerged until
the detached JetBrains surface change (PR #219) lands and this branch is rebased
onto that merge.

## Architecture contract

Invariant: for one authoritative document generation, every queue reader and
mutator chooses work through one validated identified queue graph; no effect may
silently retarget from one queue component to another.

Policy owner: `agent-doc-queue::queue_graph`. `queue_set` is a compatibility
re-export only and owns no policy.

Transition table:

| Input | Decision |
| --- | --- |
| no queue | idle |
| one unkeyed queue | legacy `default` queue |
| one keyed queue | that queue |
| multiple queues with unique explicit IDs | validate all edges, then select |
| missing/bare/empty ID in a multi-queue document | fail closed |
| duplicate/invalid ID | fail closed |
| empty/self/missing/cyclic `after=` | fail closed, including the cycle path |
| multiple ready nodes | earliest source occurrence |
| live node with a nonempty predecessor | blocked |
| predecessor refilled before the next decision | dependent blocks again |
| atomic batch generation mismatch | reject without output |
| batch creates a second queue beside a legacy queue | migrate legacy marker to `id=default` in the same proposal |

Evidence inputs: structural component spans and attributes, parsed queue
entries, source occurrence, current document generation, and the complete batch
append request. Queue identity is never inferred from prompt text.

Reactive topology: editor/CRDT/filesystem observation → authoritative document
generation → parsed `QueueGraph` → schedule decision → generation-fenced effect
→ receipt → recomputation. This change reuses the existing document-scoped
reactive lifetime and introduces no poller.

Imperative extraction audit: production head selection, continuation,
preflight maintenance, consumption, and realtime steering use the graph-selected
component. The pure actorless append API remains a one-shot transform because it
has no long-lived state; callers publish its single validated output through the
existing document mutation effect.

Allowed edit surfaces: the pure queue crate, existing queue/preflight/realtime
adapters that previously selected the first queue, queue consumption, the
singleton repair guard, and the document-format spec.

Verification: exhaustive parser/scheduler tests, strict malformed-graph tests,
dependency/refill transitions, atomic legacy migration, atomic recurring
multi-target append, focused adapter suites, then full `make check` and fresh CI
after the PR #219 rebase.

Out of scope: parallel ready-head dispatch, boolean dependency expressions,
cross-document edges, per-run queue IDs, automatic deletion of empty queue
scaffolds, version bump, release, install, or publication.

## Recurring sequence contract

Producers append all stages of a run in one `QueueAppendBatch`. For `build ->
publish`, the batch appends run N to both queues. The proposed final graph is
validated before any text is returned, so `publish` can never observe an
intermediate generation where it is nonempty but `build` has not yet refilled.
When `publish` completion adds the next run back to `build`, it must append the
next `build` and `publish` items in one batch. A refill does not revoke an
already-dispatched exact head; it affects the next schedule recomputation.
