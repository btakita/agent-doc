# Plan: route reaps a fresh start whose first cycle is a no-op (can't start an idle session)

**Pending:** `#route-reaps-idle-fresh-start`
**Status:** Deterministic fix IMPLEMENTED + unit-tested + shipped 2026-06-02 (submodule `8451143a`, both binaries rebuilt + installed). Live-gated facet (gpt-5.5 xhigh slow-cold-start end-to-end repro) remains.
**Class:** `#agent-doc-bug` — startup/route.

## Implemented (8451143a)

At the fresh-start no-ack branch (`route.rs`, after the trigger is proven
dispatched), route now re-checks the pane: if it has returned to a harness
dispatch-ready prompt the agent is alive and idle, so the first cycle was a
legitimate no-op (empty/halted queue, preflight `no_changes`) — keep the live
idle session and clear the startup-miss. Only reap when the pane is not
dispatch-ready (a genuine miss). New pure `FreshStartAckOutcome` classifier
(`CycleAcknowledged` / `IdleNoOpKeep` / `GenuineMissReap`) reuses the
already-tested `ready_prompt_candidate` discriminator; unit-tested with codex
idle-ready vs status-footer fixtures. `route::` suite 141 passed; `make check`
green. **Live-gated:** start a real idle `lazily-rs.md` codex session (empty
queue, slow gpt-5.5 xhigh cold start) and confirm the pane stays alive instead
of being reaped.

## Repro / observed (2026-06-02)

`tasks/software/lazily-rs.md` had `queue_active: false` and an **empty**
`agent:queue` (its `#lr-*` work was all completed/drained). Trying to start it:

```
agent-doc route tasks/software/lazily-rs.md
[route] Still waiting for codex (29s)... gpt-5.5 xhigh · Context 0% use
[route] Timed out waiting for codex prompt; attempting one fallback trigger injection
[route] Fallback trigger injection recovered the fresh codex start
[route] reaping startup-miss pane %75 after failed fresh route for tasks/software/lazily-rs.md
Error: fresh codex start for tasks/software/lazily-rs.md never acknowledged with a
       document cycle after trigger consumption
```

The codex **did** start (gen 21, superseding a stale gen-19 lease), consumed the
dispatched `agent-doc <FILE>` trigger, ran `preflight`, got `no_changes` (empty
queue, nothing unresolved), and therefore produced **no document cycle**. Route
interprets "no document cycle acknowledged" as a failed start and **reaps the
pane** — so the session appears "killed immediately." User symptom: *"I cannot
start the lazily-rs.md session. It is killed immediately."*

## Root cause

Route's fresh-start success criterion is "the codex acknowledged a **document
cycle** after consuming the trigger." That conflates two different things:

- a genuinely failed start (codex never became ready / never consumed the
  trigger) — should reap; vs
- a healthy start whose **first cycle is a legitimate no-op** (empty/halted
  queue, no unresolved prompt) — an **idle session**, which must be kept.

A user who just wants to OPEN a session with no pending work cannot, because the
no-op first cycle never produces the acknowledgment route waits for.

(Aggravated by gpt-5.5 xhigh's slow cold start — ~30s to a ready prompt — which
pushes the trigger-consumption past the route wait window and into the fallback +
reap path.)

## Desired behavior

1. A fresh start whose codex **proved ready** (dispatch-ready prompt) and
   **consumed the trigger** is a successful start even if the first cycle is a
   no-op. Route keeps the pane as a live idle session.
2. Reap a fresh start only when readiness/trigger-consumption itself fails (no
   prompt, no consumption within budget), not merely because the first cycle had
   nothing to do.
3. The fresh-start readiness budget must accommodate slow cold starts (gpt-5.5
   xhigh) — distinguish "still initializing" from "failed."

## Implementation sketch

- In `route.rs` fresh-codex-start path: split the success signal into
  `trigger_consumed && prompt_ready` (keep) vs `no_cycle_acknowledged` (do NOT
  reap on its own when the prior two hold and `preflight` legitimately reported
  `no_changes`).
- Treat a committed/`no_changes` `preflight` result for the dispatched trigger as
  a valid idle acknowledgment.
- Keep reaping genuine startup misses (never-ready, never-consumed) so the guard
  still catches real failures.

## Tests

- Fresh start + dispatched trigger + `preflight` returns `no_changes` (empty
  queue): assert the pane is KEPT (live idle session), not reaped.
- Fresh start that never reaches a dispatch-ready prompt within budget: assert it
  IS reaped (no regression to the real-failure guard).

## Acceptance

- A document with an empty/halted queue and no unresolved prompt can be started
  and stays up as an idle session.
- Genuine failed starts are still reaped.

## Operator workaround (until fixed)

Give the document a pending prompt before starting (type a prompt into
`agent:exchange`/`agent:queue`, or re-enable an `agent:queue auto` item) so the
fresh start's first cycle is a real cycle and route does not reap it.

## Related

- `#xdoc-route-sweep-commits-sibling-docs` (same incident chain — restoring
  lazily-rs after the contamination fix exposed this).
- `#route-starting-timeout-poll-recovery` (slow-start readiness polling).
