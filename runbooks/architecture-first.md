# Architecture-First Implementation

Development/dogfooding only. Do not copy this runbook into
`.claude/skills/agent-doc/` or any installed agent-doc skill bundle.

Use this runbook when an agent-doc development task touches shared behavior,
repeats a local fix across surfaces, asks for a refactor, or names
architecture/sprawl as the problem.

## Goal

Turn "think architecturally" into a checkable implementation contract before
editing code.

## When To Use It

- The prompt mentions architecture, sprawl, invariants, state machines, or a
  refactoring project in the agent-doc codebase.
- The bug crosses two or more agent-doc surfaces, such as route, repair,
  session-check, write, controller state, sidecars, or editor IPC.
- The intended fix would otherwise be another string match, local exception, or
  one-off recovery path.
- A prior attempt fixed one symptom but left a family of equivalent failures.

For a wholly local bug with one owner and one test surface, keep this lightweight:
name the invariant, owner, and verification in the response, then edit normally.

## Required Contract

Before code changes, identify these fields in notes, a plan file, or the
response body:

```md
Invariant:
Policy owner:
Transition table:
Evidence inputs:
Reactive topology:
Imperative extraction audit:
Allowed edit surfaces:
Verification:
Out of scope:
```

- **Invariant**: one sentence describing the rule that must hold across all
  call sites. If it cannot be stated, stop with a plan instead of editing.
- **Policy owner**: the module, state machine, or API that owns the decision.
  Other call sites may gather facts or apply effects, but they must not decide
  the policy independently.
- **Transition table**: every meaningful input state maps to a typed decision,
  including blocked, already-satisfied, stale, and retryable cases. Missing rows
  are bugs.
- **Evidence inputs**: the typed facts the owner needs. Prefer structs/enums and
parsed state over matching low-level error strings at multiple call sites.
- **Reactive topology**: name the lifecycle scope and sketch the
`Source → Computed → Effect → receipt Source` edges. Prefer this plane whenever
state changes over time or more than one caller consumes a derived fact.
- **Boundary ingress**: prefer an event channel, subscription, or watch stream
at a non-reactive/reactive boundary. Use RPC-triggered observation when no
event stream exists, and bounded polling only when the external system exposes
neither events nor a suitable trigger. Every adapter must publish typed,
generation-tagged facts into `Source`s. Polling must name the missing event
capability and publish freshness evidence. After ingress, unify the internal
agent-doc model as reactive state; do not continue polling, RPC fan-out, or
imperative recomputation between internal owners when graph edges can carry the
state.
- **Imperative extraction audit**: identify derived values that callers
imperatively recompute, caches that only update because a code path remembered
to call them, timer/poll retry loops, and duplicated generation/readiness
checks. Extract the derived values into lifecycle-scoped `Computed`s before
adding orchestration. If a direct one-shot call remains, state why it has no
long-lived state relationship and therefore no reactive edge to join.
- **Allowed edit surfaces**: the minimum modules that may change. Add a new
module only when it deletes duplicated policy, makes an impossible state
unrepresentable, or gives tests a stable boundary.
- **Verification**: pure decision tests for the policy owner plus the smallest
  integration, SimWorld, or property test that proves adapters cannot bypass it.
- **Out of scope**: the abstractions deliberately not introduced this cycle.

## Implementation Shape

- Put policy in an exhaustive enum/state-machine boundary. Adapters convert
environment facts into owner inputs and then apply the returned decision.
- Imperative adapters publish observation `Source`s. They read decisions from
`Computed`s; they do not invoke validators or recompute applicability,
authority, readiness, or convergence independently.
- Effects are idempotent, generation-fenced, and receipt-driven. Publish every
effect outcome back as a `Source` so graph invalidation—not a caller-owned
poll/sleep/retry loop—decides whether another effect is needed.
- Boundary event channels should refresh observation `Source`s directly.
RPC clients are the fallback trigger; bounded pollers are the last resort when
the external system cannot emit or trigger observations. Keep scheduling,
transport errors, and freshness metadata in the adapter; expose only typed
observations and receipts to the unified internal reactive model.
- Do not claim reactivity by creating a context per call, putting an imperative
function behind a reactive-sounding name, or requiring callers to manually
refresh the computed value. Join the existing document, turn, or process
lifecycle graph.
- Keep recovery/error text at the edge. If temporary string matching is
unavoidable, localize it to one adapter and add follow-up work to replace it
with typed evidence.
- Make already-applied, stale-capture, queue-behind-active-work, replay,
  sidecar-reset, retry, and fail-closed cases explicit when the bug class
  involves closeout recovery.
- Do not create parallel owners for the same policy. If a migration requires an
  adapter, name the old owner, new owner, and deletion path.
- Treat Lean/TLA as second-stage validation for a stable transition table, not
  as the first implementation tool for a still-implicit Rust boundary.

## Closeout Gate

The final response for an architecture-first cycle must state:

- The invariant and policy owner.
- The transition rows covered by tests.
- The reactive topology and imperative derived-state calls extracted into
`Computed`s, including any justified one-shot exceptions.
- The edited surfaces and why they are sufficient.
- Any remaining operator-verify or migration work, tagged as such rather than
  left as vague "future cleanup."

If these cannot be stated after implementation, do not mark the backlog item
done. Either finish the missing proof or gate the exact blocker.
