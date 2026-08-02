# Plan — visible-generation acknowledgement and replica rebase

## Invariant

An editor may acknowledge a controller delivery generation only from an exact
operator-visible content-hash match, and a stale local replica must be replaced
from controller canonical state before it can accept later operator deltas.
An operator edit captured after a visible controller response remains a typed
`(exact canonical base, visible result)` transition until it is published; the
adapter never advances away the only safe rebase proof.
Model turns never recreate admission by polling preflight when the binary-owned
cycle contract is absent.

## Policy owner

- `ReplicaBaselineDecision` owns the editor/replica/controller frontier
  decision in each editor adapter.
- `LocalReplicaBaselineDecision` owns direct local forwarding versus canonical
  rebootstrap of a retained operator delta.
- `src/skill.rs` owns installation of harness admission artifacts.
- The `UserPromptSubmit` hook and CP preflight pipeline own cycle admission;
  `SKILL.md` consumes their contract but does not recreate it.

## Transition table

| Visible editor | Local replica | Delivery target | Recovery | Decision |
|---|---|---|---|---|
| exact target | same target | same generation | idle | publish visible receipt |
| exact target | stale/different | same generation | idle | canonical rebootstrap, then publish receipt |
| operator result after exact target | stale/different | captured base | local delta retained | canonical rebootstrap must equal captured base, then publish delta and visible receipt |
| operator result after exact target | stale/different | different canonical | local delta retained | keep typed delta and retry fail-closed; never derive a whole-buffer edit |
| changed during rebootstrap | any | prior target | idle | reject replacement and retry |
| unrelated editor | stale/different | no exact target | idle | retain and fail closed |
| any | any | any | recovery active | retain and fail closed |
| cycle contract present | n/a | n/a | n/a | consume binary admission |
| cycle contract absent | n/a | n/a | n/a | stop as harness admission failure; never poll preflight |

## Evidence inputs

- Template-structure state of the operator-visible editor.
- SHA-256 of visible editor text, current native replica text, and each
  generation-tagged controller delivery target.
- Pending-local and recovery-in-flight state.
- Captured local base and latest operator-visible result.
- Exact editor text re-read at the atomic replacement boundary.
- Presence of the binary-emitted cycle-contract marker.

## Reactive topology

`controller delivery Source + editor projection Source + native replica Source
→ ReplicaBaselineDecision Computed → canonical replacement / visible projection
Effect → controller acknowledgement + replacement replica Sources → retained
drain invalidation`.

`operator document event Source → serialized per-document local lane → captured
(base, visible result) Source + native replica Source →
LocalReplicaBaselineDecision Computed → direct delta or exact canonical
rebootstrap Effect → controller update/visible receipt Sources`; a failed
effect retains the captured tuple behind keyed backoff.

`UserPromptSubmit Source → CP admission/preflight Effect → cycle-contract Source
→ model workflow`; there is no model-owned polling edge.

## Imperative extraction audit

- The old baseline branch recomputed “all baselines diverged” and scheduled the
  same retained retry even when the visible hash already proved the delivery.
  Replace that implicit case with the typed
  `RebootstrapVisibleRemoteTarget` decision.
- Replacement remains a one-shot effect because native registration and socket
  acknowledgement are external boundaries. Its result is fed back through the
  replacement replica, visible projection receipt, and retained drain request.
- The old local-edit branch advanced its shadow before checking the native
  baseline, so a stale replica erased the delta's base and later retries could
  only see divergence. Keep the typed base/result cut, serialize local effects,
  and require the replacement canonical text to equal that base before applying
  the minimal splice.
- Remove the skill fallback that turns a missing admission contract into a
  model-owned preflight/poll loop.

## Allowed edit surfaces

- JetBrains and VS Code replica decision/effect adapters and their tests.
- Skill installer, canonical skill/runbook instructions, and installer tests.
- Development `make install` harness so a local install projects current
  harness artifacts.
- Version/release notes and this plan.

## Verification

- Pure decision tests for exact visible target with equal versus stale replica,
  unrelated editor state, and recovery-in-flight.
- Adapter tests proving canonical replacement precedes acknowledgement, drops
  stale replica state, and rejects an editor race.
- Concurrent-edit tests proving a transient registration failure retains and
  coalesces successive queue/response edits against the original exact base.
- Skill-install tests proving Claude settings receive the CP preflight hook and
  existing hooks survive.
- Focused editor/Rust tests followed by `make check`.

## Out of scope

- No controller trust based on socket receipt alone.
- No whole-editor publication as a new CRDT mutation.
- No global polling loop; retained local work may use keyed bounded backoff.
- No force-disk recovery or automatic IDE restart.
