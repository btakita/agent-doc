# Plan: Post-recycle replica authority and exact queue-answer proof

## Operator reports

- Installing and auto-recycling agent-doc can leave attached documents registered
  but without a replica in the replacement controller. Captured responses then
  remain uncommitted because editor authority is live and disk fallback is
  correctly refused.
- A free-text queue head carrying the in-progress marker can be struck even when
  the response answered a different prompt (`#bugautostruck`).

## Confirmed causes

### Replacement recovery ran before public promotion

The replacement controller hydrates durable reliable-sync liveness while bound to
its private handoff socket. Startup recovery immediately signalled retained
editors. Those editors projected into the still-public predecessor generation.
The subsequent socket promotion replaced that controller and discarded the
freshly rebuilt process-local replica.

The durable registration survived; the process-local CRDT replica did not. This
is why reliable-sync status could report an attached editor while closeout
reported `missing_replica`.

### Selection evidence was treated as answer evidence

The queue strike projection bypassed exact response matching whenever the head
carried the in-progress marker. The marker proves which queue item was selected
for the cycle; it cannot prove that the generated response addressed it.

## Reactive design

1. A stable controller hydrates liveness after its listener is ready, then
   projects missing-replica rebuild targets asynchronously.
2. A `Preparing` replacement hydrates the same durable state but defers the
   editor effect.
3. `promote_handoff` schedules recovery keyed by the replacement pid and
   controller generation. The effect becomes runnable only after status observed
   through the public socket reports that exact stable pid+generation.
4. The existing CRDT generation/content receipt is the applied-state
   acknowledgement. The plugin does not need a second transport acknowledgement:
   controller receipt proves request admission, while CRDT observation proves
   application and convergence.
5. Free-text strike always requires the exact labeled queue-prompt quote in the
   response and stable-baseline membership. The in-progress marker remains only
   a selection fact.

## Implementation

- `agent-doc-controller-io/src/project_controller/rpc.rs`
  - defer rebuild effects for private handoff generations;
  - schedule a generation-qualified post-promotion rebuild;
  - preserve the self-promotion recovery path.
- `agent-doc-queue/src/queue_consume.rs`
  - remove the in-progress-marker answer bypass.
- `agent-doc-queue-io/src/queue_consume.rs`
  - add the `#bugautostruck` dev-harness regression.
- `SPEC.md`, `runbooks/respond.md`, and `VERSIONS.md`
  - record the authority and queue-proof contracts.

## Verification

- Exact public-generation predicate unit coverage.
- Structural controller harness coverage proving listener-before-hydration,
  preparing-generation deferral, and post-promotion recovery ordering.
- Queue dev-harness coverage proving an unrelated response cannot strike a
  selected free-text head.
- Full local `make check`, followed by local installation and live recovery of
  captured response cycles. External CI is not a closeout gate.
