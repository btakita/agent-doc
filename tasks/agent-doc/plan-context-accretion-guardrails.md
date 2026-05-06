# Plan: agent-doc context accretion guardrails

**Pending:** `#ctxacc` in `tasks/agent-doc/agent-doc-bugs2.md`
**Status:** Proposed

## Problem

`tasks/software/tsift.md` surfaced a recurring cost pattern across both Claude Code and Codex sessions:

- long-running document sessions spend most of their tokens on repeated cached context, not on assistant output
- the same document can accumulate many short `preflight -> response -> commit` cycles with little net new information
- restart churn (`fresh_restart`, Ctrl-D loops, startup misses, auto-trigger timeouts) amplifies that cost because the harness keeps reloading the same session state
- today `agent-doc` exposes the operational events, but it does not own a first-class policy for detecting or mitigating context accretion before the user pays for another large turn

This is partly a tsift observability problem, but it is also an `agent-doc` product gap. The document/session manager is the right layer to notice that a session has become churn-heavy, repetitive, or overdue for compaction and to steer the user toward a bounded recovery path.

## Goal

Add `agent-doc`-level guardrails for context accretion so long-running session documents do not silently drift into high-cost, low-signal interaction patterns.

## Desired Behavior

- `agent-doc` can detect when a document session is accreting context through repeated short cycles, large exchange tails, or restart-heavy churn
- the tool surfaces deterministic guidance before the next expensive turn, such as:
  - compact exchange now
  - restart cleanly from the current committed boundary
  - inspect a bounded digest/cost summary instead of replaying raw logs
- when the session is clearly unhealthy, `agent-doc` should warn or fail closed at the orchestration layer instead of repeatedly reenqueuing the same expensive reopen/closeout pattern
- tsift remains the preferred bounded analysis surface for cost/digest evidence when available; `agent-doc` should consume or point to it rather than duplicating transcript-analysis logic in free-form prompts

## Investigation / Implementation Plan

- [ ] Define the accretion signals `agent-doc` can measure locally without transcript replay:
  - repeated `commit_already_current` / no-op closeout loops on the same cycle
  - repeated `preflight_started -> response_captured -> commit_success` cycles within one document session
  - restart churn families already visible in runtime logs (`fresh_restart`, Ctrl-D loops, startup misses, auto-trigger timeouts)
  - exchange/body size growth and prompt-target tail length
- [ ] Decide which signals should stay tsift-only versus move into `agent-doc` policy:
  - tsift: bounded token/cost math and transcript digests
  - agent-doc: turn-time warnings, compact/restart recommendations, and fail-closed orchestration thresholds
- [ ] Design the user-facing contract:
  - when to warn only
  - when to recommend `compact exchange`
  - when to require an explicit recovery step before continuing
  - how this appears in `preflight`, `plan`, `route`, and `session-check`
- [ ] Evaluate whether `agent-doc` should call into tsift when available:
  - cheap local heuristic first
  - optional bounded tsift summary for richer diagnostics
  - deterministic fallback when tsift is absent or stale
- [ ] Add a cycle-state or ops-log summary primitive so the tool can reason about per-document churn without reparsing entire logs every turn
- [ ] Add regressions for:
  - restart-heavy document sessions that should surface a compact/restart warning
  - repeated no-op closeout loops on one document
  - long exchange tails that should trigger bounded compaction guidance
  - healthy short sessions that should not warn noisily
- [ ] Update spec/runbook surfaces only after the executable contract is pinned by tests

## Likely Files

- `src/agent-doc/src/preflight.rs`
- `src/agent-doc/src/plan.rs`
- `src/agent-doc/src/session_check.rs`
- `src/agent-doc/src/ops_log.rs`
- `src/agent-doc/src/cycle_state.rs`
- `src/agent-doc/src/route.rs`
- `src/agent-doc/tests/`
- `src/agent-doc/SPEC.md`
- `src/agent-doc/runbooks/`

## Exit Criteria

- `agent-doc` can identify churn-heavy / accreting session shapes for a document before another large turn is launched
- the tool emits bounded, actionable recovery guidance instead of allowing silent context growth
- the warning/fail-closed thresholds are deterministic and covered by regression tests
- tsift remains the bounded transcript/cost analysis backend rather than being reimplemented ad hoc inside `agent-doc`
