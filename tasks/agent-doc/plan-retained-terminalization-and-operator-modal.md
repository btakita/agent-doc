# Retained terminalization and operator-modal routing

## Invariants

1. A captured closeout that has reached the exact canonical and disk target must terminalize exactly once regardless of whether delivery convergence or `session-check` observes equality first. A false-stale retirement may reopen only the same cycle/capture/response identity and only from its named false-stale abandonment reason.
2. A routed prompt must never be injected into an operator-owned interactive modal; prompt-bearing work is queued durably and resumes only after the operator clears the modal.

## Policy owners

- Retained closeout: `agent-doc-state-backbone` owns the authoritative closeout projection; `agent-doc-cycle-state-io` emits the identity-checked false-stale reactivation fact; `agent-doc-closeout-runtime-io` owns capture settlement and commit; `agent-doc-session-check-io` must invoke it for every resumable phase before declaring terminal convergence.
- Pane routing: `agent-doc-harness` owns typed pane-blocker classification and `agent-doc-queue::route_dispatch` owns which blockers are queueable. Both dispatch-only and regular `agent-doc-route-io` paths apply the queue decision before repair, focus, interrupt, or synthesized pane input.

## Transition tables

### Retained closeout

| State | Decision |
|---|---|
| No resumable cycle | No-op |
| Retained capture; authority differs from disk | Resume/rebase through authority, then validate |
| Retained capture; authority and disk already equal the target | Clear retained intent, advance write-applied, commit exactly once |
| Matching retained capture is `Abandoned` only because the false-stale repair retired it | Emit the typed identity-checked reactivation fact, settle the same capture, then commit |
| `Abandoned` cycle/capture/hash/reason does not exactly match | Remain terminal; never reopen a generic or superseded cycle |
| Replay produces the known duplicate response/boundary shape | Normalize, then validate before terminal proof |
| Captured response is not materialized in canonical authority | Retain and retry the same capture |
| Cycle is committed or superseded | Return terminal outcome without replaying another payload |

### Routed pane

| Pane state | Prompt-bearing decision |
|---|---|
| Empty dispatch-ready composer | Inject normally |
| Active harness turn with prompt-bearing work | Queue behind owner in both regular and dispatch-only routing |
| Claude online-artifact picker | Queue behind the modal; operator dismisses with `Esc` |
| Artifact picker without prompt text to preserve | Fail with the exact `Esc` remedy; send no pane input |
| Drafted composer input or approval/review UI | Fail closed; preserve operator activity |

## Evidence inputs

- Cycle phase, capture id, response hash, abandonment reason, retained target hash/content, canonical hash, disk hash, live-editor count, and delivery convergence.
- ANSI-stripped bottom-of-pane lines. The Claude artifact modal requires both `Enter to open` and a bottom `https://claude.ai/code/artifact/` URL.
- Whether the editor supplied prompt-bearing change text that can be durably enqueued.

## Allowed edit surfaces

- `agent-doc-session-check-io`: remove the incorrect authority/disk-divergence precondition.
- `agent-doc-state-backbone`: reduce the typed identity-checked false-stale reactivation fact without permitting a generic terminal-phase regression.
- `agent-doc-cycle-state-io`: emit that fact while keeping the direct cycle projection and backbone projection coherent.
- `agent-doc-closeout-runtime-io`: recognize the projected abandonment-reason envelope and settle the retained capture through authority.
- `agent-doc-harness`: classify the Claude artifact picker once.
- `agent-doc-queue`: map that typed blocker to a queue source.
- `agent-doc-route-io`: apply the queue-before-repair decision in regular and dispatch-only routes.
- `agent-doc-controller`: render the exact operator remedy at the edge.
- Version metadata and this architecture record.

## Verification

- Pure tests for exact false-stale identity reactivation, equal-target capture resumption, artifact-picker classification, queue-source mapping, and recovery wording.
- Package integration tests across session-check, harness, queue, route/controller compilation.
- Full `make check`, including simulation, Lean, and TLA gates.
- Live dogfood: install/recycle, then prove the retained `agent-doc-bugs2.md` capture commits through `session-check` without another payload.

## Out of scope

- Sending `Esc`, `Enter`, or other synthetic input to dismiss operator UI.
- A generic visual/modal classifier for every future Claude UI surface.
- JetBrains plugin JAR changes; these transitions are binary-owned.
