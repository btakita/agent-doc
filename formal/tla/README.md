# TLA+ / PlusCal model checks

The models are authored as concurrent PlusCal algorithms or direct TLA+ state
machines and checked in a temporary directory on every test run.

`AgentDocCloseout.tla` checks:

- exact retained-response safety across replay and bounded ACK failures;
- preservation of steering added after the original response capture;
- semantic response projection over the current editor cut, including idempotent
  post-cell replay that preserves queue tombstones and a single boundary marker;
- response-cell settlement without requiring whole-document lineage, while
  non-response semantic rebases remain causally fenced;
- separation of native-library, installed-package, and live-editor generations;
- adoption of a plugin generation registered after the turn's preflight; and
- eventual package convergence, editor publication, and response commit under
  the PlusCal process fairness assumptions.

`PassiveTmuxSync.tla` checks the exact-visible editor-tab sync boundary:

- authoritative actor lookup is available only inside the owning Project
  Controller and never from a standalone safe-passive request;
- a controller-local proof permits the target actor to atomically swap with the
  stale visible actor while preserving a unique visible/stashed partition;
- neither request autostarts an actor; and
- fair execution eventually applies the controller-local request and blocks the
  external request.

`PaneExecutionAuthority.tla` checks preflight/write/controller admission across
live, stale, unknown, absent, headless, rebound, and generation-mismatched pane
ownership:

- a non-owner generation never mutates document state;
- rejection has no cycle, lease, or write side effect;
- at most one owner generation mutates;
- stale-owner repair requires exact live process ownership and never seizes a
  live owner; and
- an admitted recovery reaches terminal settlement under weak fairness.

`SupervisorGenerationTransition.tla` checks install/restart/recycle admission:

- an uncaptured open cycle is never replaced;
- no replacement occurs while supervisor IPC is unsafe;
- only captured-response recovery may cross an open cycle;
- at most one replacement occurs; and
- a pending request eventually replaces the supervisor after the cycle closes
  and IPC drains under weak fairness.

`ReactiveTopology.tla` checks the shared lifetime-scoped graph contract used by
agent-doc's Lazily state machines:

- a derived generation and its receipt never lead the observation Source;
- every mutation and receipt has an exact observation/effect lineage;
- stale effects and effects from a closed scope never mutate;
- a generation mutates at most once; and
- after observations quiesce, an open graph eventually publishes the matching
  receipt under weak fairness.

This is a compositional proof boundary, not a universal proof of arbitrary I/O.
The Rust scope guard connects production graph construction to the modeled
lifetime rule; domain-specific models remain responsible for their transition
tables and external-system assumptions.

`CrdtLineageFence.tla` exhaustively checks the finite recovery control state:

- queue tombstones and editor-authored deletions never regress;
- the latest operator frontier retracts deleted heads from the Lazily lineage, while a
  clean crash-lost add remains durable and eventually recovers;
- a whole-document replacement preserves durable pending agent intent;
- stale-lineage frames cannot corrupt or resurrect canonical content;
- quarantined stale frames eventually advance the ACK cursor; and
- delivery ACK and native editor save are distinct transitions;
- a replay-created unmatched component close is normalized before commit, while
  the operator cut and queue deletion tombstones remain unchanged;
- a replay-created second exchange boundary is an explicit transient that must
  normalize to the latest frontier before commit, without changing the
  operator cut or queue deletion tombstones;
- an operator advance between save request and save invalidates the old proof
  without losing the durable agent intent; and
- commit is impossible until the retained agent intent is applied and the exact
  still-current editor version is saved to disk.

`VisibleDeliveryReceipt.tla` checks the write-side dual of
`EditorReplicaStrand`: the visible-delivery receipt that gates the editor-native
save. `RelayHub` derives availability (`delivery_converged`) and the receipt
(`visible_delivery_projected`) over the live membership cut, and the
non-convergence budget releases a stalled replica from only the first of them.
The module checks:

- an availability release is never a receipt, so the tempting collapse of the
  two predicates is rejected rather than adopted as a fix;
- a native save is authorized by the receipt alone;
- a replica leaves the delivery cut only on proof that its endpoint answered and
  refused, so the detached-write path is never a disguised `--force-disk`;
- a replica still inside its convergence budget is never preempted; and
- the canonical cut eventually reaches disk with no operator action.

It has one wedge config PER KNOB, because the two fixes are independent and a
single wedge would let one go vacuous the moment the other landed:

- `VisibleDeliveryReceiptWedge` removes the drop edge and must deadlock — the
  production wedge where a refused replica held the receipt false forever, the
  native-save gate never opened, and every later cycle was refused admission;
- `VisibleDeliveryReceiptBuildMismatchWedge` keeps the drop edge but declines to
  classify an exhausted build mismatch as definitive, and must also deadlock. The
  endpoint answered with a typed version rejection AND the one recovery for that
  rejection failed to deliver, yet it stayed "retryable" forever, so nothing ever
  proved it had stopped serving.

Its reach config asserts the negation of a save through a still-serving live
editor and must be violated, proving the fix did not quietly route every document
through the detached-write path instead.

`PlanClosureContract.tla` checks what a dispatch plan may assert was completed.
The other modules model liveness wedges — a reachable state with no outgoing
transition. This one models the opposite: a transition enabled when it should
not be, whose effect is unrecoverable. A wedge costs time; a false closeout
marks unexecuted work complete and destroys the evidence it was never done. It
checks:

- closeout never completes an id the turn did not dispatch;
- an `[operator-verify]` id, which no agent turn can execute, is never closed;
- queue residue is not dispatch — "present in the component" earns no `--done`;
  and
- a genuinely dispatched head still closes out (the reach obligation, so the
  safety invariants cannot pass vacuously by never emitting `--done` at all).

Its wedge config derives the contract from the queue prose instead of the
resolved activity and dispatch state, and must violate — each of the three
safety invariants independently, which is how the production report read: on a
queue preflight had already resolved inactive with zero drainable heads, the
plan emitted three `--done` flags, one of them for an `[operator-verify]` item.

`RetainedProjectionHold.tla` checks the JetBrains replica's registration
decision when the published shadow, the live buffer, and canonical all differ:

- registration never replaces operator text canonical has not seen; and
- once canonical holds every operator edit, the replica attaches even when a
  reloaded disk projection moved binary-owned markers (`(HEAD)`, boundary) and
  canonical carries merge debris.

`RetainedProjectionHoldWedge.cfg` keeps the markers inside the containment proof
and must violate `EventuallyRegistered` (the 2026-09-29 `fpe.md` hold);
`RetainedProjectionHoldReach.cfg` must show the containment edge is taken.

`RetainedTransitionFixedPoint.tla` checks that a retained write whose
Base -> Target delta is already in the editor's cut settles as that fixed point
instead of being rebased again: at most one copy of the response, and the
write always settles. `RetainedTransitionFixedPointWedge.cfg` (whole-text
comparison only) must violate `AtMostOneResponse`, the 2026-09-29 `fpe.md`
duplicate; `RetainedTransitionFixedPointReach.cfg` must show the fixed-point
edge is taken.

`RealtimeSteeringStop.tla` checks that operator steering landing after a commit
is always handed back to the agent in the current turn, whether it is a new
prompt (`prompt_target`) or typing inside an existing queue item or prompt
(`content_edit`). `RealtimeSteeringStopWedge.cfg` (prompt_target only) must
violate `SteeringEventuallyAnswered`, the 2026-09-29 `api.md` stop;
`RealtimeSteeringStopReach.cfg` must show a content_edit is handed back.

`EditorAuthorityLadder.tla` checks the canonical-state ladder (several editors
open: their reconciliation; one editor: its buffer; none: disk) with the
in-memory CRDT as a forward-only merge engine: no open editor ever loses text
nobody deleted. `EditorAuthorityLadderWedge.cfg` (registration adopts the CRDT,
handoff reseeds from disk) must violate `NoRollback`, the 2026-09-29 `api.md`
rollback; `EditorAuthorityLadderReach.cfg` must show agent writes still reach
editors. `formal/authority_ladder/EditorAuthorityLadder.lean` proves the same
safety property for every instance, not only the bounded ones TLC explores.

`ConflictReconciliation.tla` checks where text lands when the operator's live
edit and the agent's merge meet (`#editorauth1`): same-point appends put the
agent's content first with the cursor at the end of the operator's edit, edits
of independent regions both apply, and a same-span conflict is surfaced in the
buffer with the text both sides share kept outside the marks, resolving to
either side exactly. The operator keeps typing at the cursor afterwards.
`ConflictReconciliationOperatorFirstWedge.cfg` (operator's append first) must
violate `SamePointAgentFirst`, `ConflictReconciliationDropWedge.cfg` (last
writer wins on an overlap) must violate `AgentPreserved`, and
`ConflictReconciliationReach.cfg` must show a conflict is actually surfaced.
`formal/authority_ladder/ConflictReconciliation.lean` proves the same merge for
every base and every pair of edits.

`AdmissionSplitMerge.tla` checks preflight's admission three-way merge ladder
(`#admissionmergedup`): when the editor authority and disk both advanced past
the baseline, the adopted revision never holds a list item more times than an
identity merge would. The runtime guard every rung must clear is count
conservation (at most baseline + each side's additions), because the
line-based `Semantic` rung keeps both of two different edits of one item.
`AdmissionSplitMergeWedge.cfg` (the pre-fix ladder) must violate
`NoDuplicateItem`, the 2026-09-30 `infra.md` duplicate;
`AdmissionSplitMergeGuardOffWedge.cfg` shows ending the ladder on
`equals_authority` is no substitute for the guard; `AdmissionSplitMergeReach.cfg`
must show disk-only additions still land.

`StaleColumnRecycle.tla` checks GH #136: the stale-supervisor column gate and
the safe-boundary recycle request it depends on. The editor link is an
adversarial channel (delay, reorder, drop, duplicate, reconnect) and the
controller applies any delivered layout in any order; the request's
consumption bound is an arbitrary local `Expire` event, so no timeout appears
in a safety argument. Safety: a stale pane is never promoted out of the stash
while its request is overdue (`NoStashPromotionWhileOverdue`), a stash
promotion never widens the target window (`StashPromotionNeverWidens`), and
once refused as overdue it is not promoted again until consumed
(`NoFlapAfterOverdueRefusal`). Liveness (`EventuallyConsumed`) holds only
under fairness of the supervisor's own idle boundary and cycle closure, for a
durable level-triggered request. One wedge per fix must violate:
`StaleColumnRecycleOverdueWedge`, `StaleColumnRecycleWidenWedge`,
`StaleColumnRecycleRefreshWedge` (a refresh restarting the consumption clock),
`StaleColumnRecycleLapseWedge` (an install fan-out request that lapses), and
`StaleColumnRecycleDropWedge` (a fire-and-forget notification under Drop);
`StaleColumnRecycleReach` and `StaleColumnRecycleConsumeReach` keep the focus
exception and consumption reachable. The editor link is the shared
`NetChannel` (`#netadv3` port): publications carry no id, so two equal
publications are two copies in the bag, and the pre-fix notification rides the
same channel under `FairLossy({Notify})`. The Drop wedge is therefore the
channel's own `Drop`, not a model-local action.
## Adversarial network: `NetChannel`

Most models above collapse "A sends, B receives" into one atomic transition,
which silently assumes the network is reliable, ordered and instantaneous.
`NetChannel.tla` is a library module that makes the network an explicit
adversary instead (`#netadv2`, plan: `tasks/agent-doc/plan-network-adversarial-correctness.md`).

- `net` is a bag (multiset) of in-flight messages, so any in-flight message may
  be delivered next: delay and reordering are free.
- Adversary actions, with no fairness at all: `Drop` (loss, including a silent
  half-open proxy stall), `Duplicate`, and `Reconnect`, which bumps the
  connection generation `gen` and discards any subset of in-flight messages.
  The rest survive and arrive later, stale.
- `MaxCopies` caps copies per message (an extra copy coalesces, which is just a
  Drop) and `MaxGen` caps reconnects, so state spaces stay finite. Drop is
  unbounded.
- `FairLossy(Msgs)` is the fair-lossy assumption: a message put in flight
  infinitely often is eventually delivered. It is strong fairness per message,
  because Drop interrupts enabledness between resends, so weak fairness would
  never fire. It promises nothing for a message sent finitely often.

### Using it from a model

```tla
VARIABLES net, gen, delivered, ...your vars...
C == INSTANCE NetChannel          \* binds MaxCopies, MaxGen, net, gen, delivered

Init == C!ChannelInit /\ ...
\* send:    C!Send(m)                    /\ your update
\* receive: C!Deliver(m)                 /\ your handler       (consume only)
\*          C!DeliverAndSend(m, reply)   /\ your handler       (e.g. data -> ack)
Next == \/ ... \/ \E m \in C!InFlight : Recv(m)
        \/ (C!Adversary /\ UNCHANGED yourVars)
        \* or (C!Reconnect /\ OnReconnect) to resync from durable state
Spec == Init /\ [][Next]_vars /\ WF_vars(Resend) /\ C!FairLossy(Msgs)
TypeOK == C!ChannelTypeOK(Msgs) /\ ...
```

Rules the module relies on:

- Every step either uses one `C!` channel action or leaves
  `C!chanVars` (`net`, `gen`, `delivered`) unchanged.
- The receive action for a message is enabled whenever that message is in
  flight. A receiver may discard a stale or duplicate message, but it must take
  it off the wire, or `FairLossy` would rule out behaviours that really happen.
- Stamp messages with `gen` yourself and compare on receipt. Safety must hold
  under `[][Next]_vars` alone. Fairness is only for liveness, and progress needs
  a fair resend (or a pull/resync) because the channel never retries for you.
- To migrate an atomic IPC step `A ==> B`, split it into a send action and a
  receive action keyed by an idempotency key (sequence number or generation).
  The receiver keeps that key in durable state that survives `Reconnect`.

`NetChannelRetransmit.tla` is the reference protocol and template:
resend-until-ack with a receiver that applies at most once per
(sequence, generation). Under the fully adversarial channel it proves
`ExactlyOnce`, `NoStaleApply` and `AckedImpliesApplied`. Under `FairLossy`
plus a weakly fair resend it proves `Completes`. Each wedge is must-violate:

- `NetChannelRetransmitFireAndForgetWedge` (send once, `MaxGen = 0`): one Drop
  stalls it, so `Completes` is violated;
- `NetChannelRetransmitDuplicateWedge` (non-idempotent receiver): a Duplicate is
  applied twice, so `ExactlyOnce` is violated;
- `NetChannelRetransmitStaleGenWedge` (no generation check): a message that
  survived a Reconnect is applied, so `NoStaleApply` is violated;
- `NetChannelRetransmitUnfairWedge` (no `FairLossy`, fair resend only): the
  adversary drops every resend forever, so `Completes` is violated, which shows
  the liveness proof rests on the channel assumption;
- `NetChannelRetransmitReach` must be violated: the positive run completes
  after the adversary really did duplicate a message and carry one across a
  reconnect.

`scripts/run_tla.sh` copies library modules (`libraries=(NetChannel)`) next to
the models that instance them.
## Hot-path models over `NetChannel` (`#netadv3`)

Each module below re-checks a hot-path protocol with every cross-process step
split into a send and a receive over `NetChannel`. Safety is checked under
`[][Next]_vars` alone (full adversary, no fairness). Liveness is checked under
`FairLossy` plus weak fairness on the protocol's own (re)send actions. Where
strong fairness per message is too slow over two versions or a reconnect, the
liveness run uses smaller bounds and a separate `*Safety` config (listed in
`must_pass` in `scripts/run_tla.sh`) checks the invariants with the larger ones.
The atomic modules stay: they prove each protocol's decision table, and the Net
modules prove the transport around it.

| Module | Hops | Wedges (each MUST violate) |
|---|---|---|
| `VisibleDeliveryReceiptNet` | wake, projection ACK, recovery request with refused/`deferred` answers, native save/receipt | `WakeOneShot` (F9), `RecoveryLatch` (F10), `SaveOneShot` (F13/F12), `StaleAck` (receipts keyed by content), `TimeoutRefusal` (R2: a timeout or `deferred` read as a refusal; fixed by `#netadv5`) |
| `EditorReplicaStrandNet` | re-registration request with refused/accepted/`deferred` answers | `Latch` (ERS-1: a budget whose receipts were all lost latches self-heal forever), `GiveUpRefusal` (R2/R3 class, fixed by `#netadv5`) |
| `RecycleSettleDispatchNet` | settle-wait RPC, one request per connection | `Unreachable` (RSD-1: two lost round trips refused a pending recycle), `Ttl` (R9: TTL read as proof; fixed by `#netadv5`, abandonment needs the supervisor gone) |
| `AgentDocCloseoutNet` | closeout owner claim and heartbeat | `OneShotClaim` (F4), `HeartbeatBreak` (F5a), `IgnoreLoss` (F5b), `Fence` (ADC-fence, not fixed: needs a commit fenced by owner id; `FencedTarget` is the passing target) |
| `PassiveTmuxSyncNet` | editor focus observation and its reply | `AdvanceFirst` (F14/F15: tracking advances before the swap), `Reorder` (sequence fence) |
| `LifecycleSequence` | lifecycle, heartbeat and queue-control level updates | `Reorder` (SIM-F1/SIM-F2: generation-only fence, last arrival wins) |

Each module also has a reach config that must be violated, so the happy path
stays reachable over the lossy channel.

The atomic `RecycleSettleDispatch` no longer treats an elapsed TTL as a fact
(`#netadv5` R9): abandonment needs the supervisor to be gone, and past the TTL
with it alive the gate refuses with a retryable verdict
(`RefuseOwnerStillRecycling`). `RecycleSettleDispatchTtlWedge` restores the old
reading and must violate `NeverProceedsPastALiveRecycle`.

## Network channel assumptions

Audit `#netadv1`, 2026-10-05. The deployment design point is a Coder remote
workspace (JetBrains Remote Dev) behind Zscaler. That means high and variable
latency, silent half-open stalls, and lost messages. The full ranked audit of the
code-side protocols is in
[`docs/reference/network-channel-audit.md`](../../docs/reference/network-channel-audit.md).

**Summary: no model proves safety over a lossy, delayed or stalling channel.** No
model has a sequence or bag channel. None has an in-flight state where a request
is outstanding with no answer, and none has a stale reply arriving after a newer
request. Almost every cross-process receipt is one atomic action, usually a read
of a variable the other party owns. That is an atomic-step assumption even where
the prose says "message". Lines are `file:line` in this directory unless noted.

Legend for the channel column:

- **AS**: an atomic step. Sender and receiver change state in one action.
- **SV**: the receiver reads the sender's state variable directly. This is still an atomic-step assumption.
- **1-slot**: a single boolean mailbox that is delivered under weak fairness. It is a reliable channel and cannot lose a message.
- **none**: no cross-process interaction is modelled.

For the D/Du/R/Rc column (Drop, Duplicate, Reorder, Reconnect): `y` = yes,
`p` = partial, `-` = no.

| Model | Cross-process interaction: channel (cite) | D / Du / R / Rc | Constant that a safety property depends on | Safety proven only for a perfect network? |
|---|---|---|---|---|
| AdmissionSplitMerge | none. Fixed auth+disk snapshots in `Init` (106-113) | - / - / - / - | none | n/a. Assumes one consistent snapshot |
| AgentDocCloseout | installer→editor `installCompleted` SV (37→47); editor restart SV (50→62); `RevalidateGeneration` SV (64). The ack is a local countdown, then `acked := TRUE` (67-74) | p / - / - / p | `MaxAckFailures` (29): after it runs out, the ack is decreed | **yes**. The ack always lands |
| CloseoutChurn | all SV awaits. `ackLanded=TRUE` at Init (28); `replacementAckQueueEmpty := TRUE` by decree (124) | - / - / - / p | none | **yes**. A straight-line script |
| ConflictReconciliation (+ Lean) | none. Merge is a pure function (133-138; `ConflictReconciliation.lean:95`) | - / p (identical edit applies once, 104-105) / - / - | none | n/a |
| CrdtLineageFence | frames: 1-slot `staleFramePending`/`currentFramePending` (24-25, 86-105); `EditorNativeSave` `disk' = canonical` AS (128-132); `Commit` SV (209) | p (crash loss 150-167) / p (replay 169-187) / p (old-lineage frame 86-89) / p (lineage rotation 76-84) | none | **mostly**. Frames are never lost; save and commit are atomic cross-party reads |
| EditorAuthorityLadder (+ Lean) | `Forward`/`Deliver` SV set-union syncs (118-121, 142-145); `RegisterForward` atomic exchange (149-154); `Handoff` reads every editor at once (167-169, 81-82); `deleted` is a **global, instantly visible tombstone set** (60) | implicit / implicit / implicit / p | none | Safe against skipped or repeated *state* syncs. **Not** safe once tombstones or the handoff cut can be stale |
| EditorReplicaStrand | `Reregister*` request+answer is one AS reading endpoint `mode` (185-226) | p (permanent `"silent"`, 221-226) / - / - / p | `MaxReregisterAttempts` (108) gates demotion (246, 277) | **yes**. A slow-but-serving endpoint is not expressible |
| IpcBuildIdentity | `Handshake` compares both identities in one step (131-137) | - / - / - / - | none | n/a. Assumes the identity arrives intact |
| JetBrainsFileCache | reregister copies editor→canonical SV (104-108); save copies editor→disk SV (114-121); every request answered (96-99) | - / - / - / p | none | **yes** |
| PaneExecutionAuthority | `Mutate` is an atomic check-and-set on owner (153-155); one rebind race (57-69) | - / - / - / p | one race only (`rebindUsed` 59) | yes (local IPC) |
| PassiveTmuxSync | `actorBinding` SV with atomic swap (30-37) | - / - / - / - | none | yes (local) |
| PlanClosureContract | shared sets; `Closeout` reads `planned` (129-131) | - / - / - / - | none | n/a |
| ReactiveTopology | in-process; effect + receipt in one action (73-88) | - / p / p (generation fence 78, 90-96) / - | none (MaxGeneration bounds the environment) | n/a (in-process) |
| RealtimeSteeringStop | `StopHook` reads `steering` SV (70-77) | - / - / - / - | none | n/a |
| RecycleSettleDispatch | settle-wait RPC = SV read of supervisor `recycle` that always returns by `WaitBudget` (91-99) | - / - / - / p | **`Ttl`, `WaitBudget`, `ASSUME Ttl > WaitBudget` (44-56)**; `Abandoned` treated `age > Ttl` as proof of loss (114-119). **Updated `#netadv3`:** abandonment now needs the supervisor gone; the TTL only ends a wait in a retryable refusal. Network re-model: `RecycleSettleDispatchNet` | yes for the transport (see the Net module) |
| RefusedSaveOperatorAction | `RecoveryPass` request + instant nondeterministic answer (129-135) | p (`delivery_failed_to_all`, 94) / - / - / - | `MaxPasses` (88), for finiteness | yes. Every refusal is assumed authentic |
| ResponseCheckpoint | `visibleSeq := producedSeq` instantly visible (42-44); `Seal` SV (53-54) | - / p (46-48) / - / - | none | **yes** |
| RetainedProjectionHold | `ControllerIngest` SV (139-144); `Register` decides over buffer+shadow+canon atomically (120-125, 166-179) | p (lag, forced by WF 191) / p (157-162) / - / p | none | **yes** |
| RetainedTransitionFixedPoint | `Settle` reads the editor cut atomically (97-109) | - / y (the subject) / - / - | none | **yes**. A stale cut read is exactly the duplicate it prevents |
| StopHookContinuation | local shared state | - / - / - / - | none | n/a |
| StopHookFailClosed | local | - / - / - / - | none | n/a |
| SupervisorGenerationTransition | `DrainIpc` is a boolean oracle; no in-flight messages (47-53) | - / - / - / p (63-73) | none | **yes**. "Drained" is decreed |
| TransientRefusalLatch | `Served`/`Refused*` read `responder` SV (181-222); crash = immediate EOF (149-154, 204-210) | p / - / - / y (163-175) | **`MaxRetryAttempts`=3, `MaxFaults`=2** (102-103, cfg). `OnlyAuthoredVerdictsStop` (330) relies on MaxFaults < MaxRetryAttempts | **yes**. No outstanding-forever request |
| VisibleDeliveryReceipt | 1-slot `pending` (102); `AckDelivery` applies the ack and drains the hub in one AS (192-199); recovery answers SV (213-261); `NativeSave` SV over hub+endpoint (287-294) | p (`PullWithoutAck` 182-188; a lost-after-apply ack cannot occur) / p (178-181) / - / p (disconnect 269-280, no reconnect) | **`MaxNonconvergence`** (97) via `HoldsBarrier` (120-123) gates the drop edge (275) | **yes**. Answers are instant and truthful |
| `capture_closeout/CaptureCloseout.lean` | none. Pure `decide*` functions | p (late ack becomes `retainedForAsyncDelivery`, 94-110) / - / - / p (151-165) | `decideRetry` backoff (137-143) | n/a |
| `wait_machine/WaitMachine.lean` | signals are `step` inputs (105-134) | - / - / - / - | **`globalHangCeiling = 10000` ms (41)**, `reinstallBudget` (44). `no_hang` (175) and `fail_closed_at_ceiling` (225) are bounded-time safety | n/a. Above ~10 s of latency, IPC-ack waits fail closed by design |
| `wait_machine/RealtimeCycle.lean` | `acknowledge` sets `acked := s.visibleExact` (133), an SV read of the editor | - / p (`save_idempotent` 144) / - / - | none | **yes** for the ack. Faults live only in SimWorld `realtime_ipc_cycle_model` (`src/sim_world.rs`) |

Cross-cutting gaps that netadv2/netadv3 must close:

1. **A received refusal is believed.** EditorReplicaStrand (241-248),
   VisibleDeliveryReceipt (226-233, 269-280) and TransientRefusalLatch
   (`RefusedAuthored`, 216) each turn a refusal into irreversible demotion or a
   drop. No model has a proxy error, a forged reset, or a stale reply to an
   older request generation.
2. **There is no half-open stall.** The only silence is EditorReplicaStrand's
   permanent `"silent"` mode. Every other wait returns, either instantly or at a
   budget.
3. **Safety depends on a constant** in TransientRefusalLatch, RecycleSettleDispatch,
   WaitMachine, VisibleDeliveryReceipt, EditorReplicaStrand and AgentDocCloseout.
   This conflicts with the target property "no timeout value may be part of a
   safety argument".
4. **Vacuous invariants.** In each of these the variable is never written:
   `RetainedResponseIsUnique` (AgentDocCloseout:27), `SingletonBoundary`
   (JetBrainsFileCache:71), `StrictUnmarkedNeverMutates` (CloseoutChurn:54),
   `PassiveSyncNeverAutostarts` (PassiveTmuxSync:25).
5. **No liveness proof covers loss.** Liveness rests on weak fairness of the
   delivery or answer actions themselves, not on re-sending over a lossy channel.

Run `make tla`. Set `TLA_TOOLS_JAR=/path/to/tla2tools.jar` to use an existing
TLA+ tools installation. Otherwise the runner downloads the pinned upstream
artifact into `target/tla/` and verifies its SHA-256 digest.
